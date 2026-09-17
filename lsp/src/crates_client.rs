use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use semver::Version;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::popular_crates::search_popular_crates;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SparseIndexLine {
    pub name: String,
    pub vers: String,
    #[serde(default)]
    pub yanked: bool,
    pub pubtime: Option<String>,
    #[serde(default)]
    pub features: HashMap<String, Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrateVersionInfo {
    pub version: String,
    pub parsed: Option<Version>,
    pub yanked: bool,
    pub pubtime: Option<String>,
    pub is_prerelease: bool,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CrateMetaResponse {
    #[serde(rename = "crate")]
    pub krate: CrateMeta,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CrateMeta {
    pub id: String,
    pub name: String,
    pub max_version: String,
    pub description: Option<String>,
    pub homepage: Option<String>,
    pub documentation: Option<String>,
    pub repository: Option<String>,
    pub downloads: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SearchApiResponse {
    pub crates: Vec<SearchCrateItem>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SearchCrateItem {
    pub name: String,
    pub max_version: String,
    pub description: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum VersionStatus {
    UpToDate {
        current: String,
        latest: String,
    },
    Outdated {
        current: String,
        latest: String,
        newer_versions_count: usize,
    },
    PreRelease {
        current: String,
        latest_stable: String,
    },
    NotFound {
        current: String,
    },
    Invalid {
        current: String,
    },
}

#[allow(dead_code)]
#[derive(Clone)]
pub struct CratesClient {
    http: reqwest::Client,
    cache_versions: Arc<RwLock<HashMap<String, (Instant, Vec<CrateVersionInfo>)>>>,
    cache_meta: Arc<RwLock<HashMap<String, (Instant, Option<CrateMeta>)>>>,
    cache_ttl: Duration,
}

impl CratesClient {
    pub fn new() -> Self {
        let http = reqwest::Client::builder()
            .user_agent("crate-helper-zed-lsp (https://github.com/parvez/crate-helper)")
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap_or_default();

        Self {
            http,
            cache_versions: Arc::new(RwLock::new(HashMap::new())),
            cache_meta: Arc::new(RwLock::new(HashMap::new())),
            cache_ttl: Duration::from_secs(600), // 10 minutes cache
        }
    }

    /// Convert crate name to sparse index path (RFC 2789)
    pub fn sparse_index_path(name: &str) -> String {
        let lower = name.to_lowercase();
        match lower.len() {
            1 => format!("1/{lower}"),
            2 => format!("2/{lower}"),
            3 => format!("3/{}/{lower}", &lower[0..1]),
            _ => format!("{}/{}/{lower}", &lower[0..2], &lower[2..4]),
        }
    }

    /// Fetch all versions of a crate from the official crates.io sparse index
    pub async fn get_versions(&self, crate_name: &str) -> Result<Vec<CrateVersionInfo>> {
        let lower_name = crate_name.to_lowercase();

        // Check memory cache
        {
            let cache = self.cache_versions.read().await;
            if let Some((inserted, versions)) = cache.get(&lower_name) {
                if inserted.elapsed() < self.cache_ttl {
                    return Ok(versions.clone());
                }
            }
        }

        let path = Self::sparse_index_path(&lower_name);
        let url = format!("https://index.crates.io/{path}");

        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .context("failed to fetch from sparse index")?;

        if !resp.status().is_success() {
            anyhow::bail!("Crate '{}' not found in sparse index ({})", crate_name, resp.status());
        }

        let body = resp.text().await.context("failed to read index response")?;
        let mut versions = Vec::new();

        for line in body.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Ok(entry) = serde_json::from_str::<SparseIndexLine>(line) {
                let parsed = Version::parse(&entry.vers).ok();
                let is_prerelease = parsed.as_ref().map_or(false, |v| !v.pre.is_empty());
                versions.push(CrateVersionInfo {
                    version: entry.vers,
                    parsed,
                    yanked: entry.yanked,
                    pubtime: entry.pubtime,
                    is_prerelease,
                });
            }
        }

        // Sort versions descending (semver where possible, non-yanked first)
        versions.sort_by(|a, b| {
            // First compare yanked (non-yanked preferred)
            if a.yanked != b.yanked {
                return a.yanked.cmp(&b.yanked);
            }
            // Compare parsed semver descending
            match (&a.parsed, &b.parsed) {
                (Some(va), Some(vb)) => vb.cmp(va),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => b.version.cmp(&a.version),
            }
        });

        // Store into cache
        {
            let mut cache = self.cache_versions.write().await;
            cache.insert(lower_name, (Instant::now(), versions.clone()));
        }

        Ok(versions)
    }

    /// Fetch metadata for a crate from crates.io API
    #[allow(dead_code)]
    pub async fn get_metadata(&self, crate_name: &str) -> Option<CrateMeta> {
        let lower_name = crate_name.to_lowercase();

        // Check cache
        {
            let cache = self.cache_meta.read().await;
            if let Some((inserted, meta)) = cache.get(&lower_name) {
                if inserted.elapsed() < self.cache_ttl {
                    return meta.clone();
                }
            }
        }

        let url = format!("https://crates.io/api/v1/crates/{lower_name}");
        let meta = match self.http.get(&url).send().await {
            Ok(resp) if resp.status().is_success() => {
                resp.json::<CrateMetaResponse>().await.ok().map(|r| r.krate)
            }
            _ => None,
        };

        {
            let mut cache = self.cache_meta.write().await;
            cache.insert(lower_name, (Instant::now(), meta.clone()));
        }

        meta
    }

    /// Search crates (uses local popular list + crates.io search API)
    pub async fn search_crates(&self, query: &str) -> Vec<SearchCrateItem> {
        let mut results = Vec::new();
        let mut seen = std::collections::HashSet::new();

        // 1. Instant local popular crates
        for pop in search_popular_crates(query) {
            seen.insert(pop.name.to_string());
            results.push(SearchCrateItem {
                name: pop.name.to_string(),
                max_version: "".to_string(),
                description: Some(pop.description.to_string()),
            });
        }

        // 2. Query crates.io API for broader search if query is not empty
        if !query.trim().is_empty() {
            let url = format!(
                "https://crates.io/api/v1/crates?q={}&per_page=10",
                urlencoding_query(query)
            );
            if let Ok(resp) = self.http.get(&url).send().await {
                if let Ok(api_res) = resp.json::<SearchApiResponse>().await {
                    for item in api_res.crates {
                        if !seen.contains(&item.name) {
                            seen.insert(item.name.clone());
                            results.push(item);
                        }
                    }
                }
            }
        }

        results
    }

    /// Check if current version is up to date or backdated (outdated)
    pub fn check_version_status(
        current_req_str: &str,
        versions: &[CrateVersionInfo],
    ) -> VersionStatus {
        let clean_req = current_req_str.trim().trim_matches('"');
        if clean_req.is_empty() {
            return VersionStatus::Invalid {
                current: clean_req.to_string(),
            };
        }

        // Find latest stable non-yanked version
        let latest_stable = versions
            .iter()
            .find(|v| !v.yanked && !v.is_prerelease)
            .or_else(|| versions.iter().find(|v| !v.yanked))
            .or_else(|| versions.first());

        let latest_version_str = match latest_stable {
            Some(v) => &v.version,
            None => {
                return VersionStatus::NotFound {
                    current: clean_req.to_string(),
                }
            }
        };

        // If exact string matches latest version
        if clean_req == latest_version_str {
            return VersionStatus::UpToDate {
                current: clean_req.to_string(),
                latest: latest_version_str.to_string(),
            };
        }

        let latest_v = match latest_stable.and_then(|v| v.parsed.as_ref()) {
            Some(v) => v,
            None => {
                return if clean_req == latest_version_str {
                    VersionStatus::UpToDate {
                        current: clean_req.to_string(),
                        latest: latest_version_str.to_string(),
                    }
                } else {
                    VersionStatus::Outdated {
                        current: clean_req.to_string(),
                        latest: latest_version_str.to_string(),
                        newer_versions_count: 1,
                    }
                };
            }
        };

        // If valid SemVer Version (e.g. "1.0.100" or "^1.0.100" or "=1.0.100")
        let clean_no_caret = clean_req.trim_start_matches('^').trim_start_matches('=');
        if let Ok(current_v) = Version::parse(clean_no_caret) {
            if &current_v < latest_v {
                let newer_count = versions
                    .iter()
                    .filter(|v| !v.yanked)
                    .filter_map(|v| v.parsed.as_ref())
                    .filter(|v| *v > &current_v)
                    .count();

                return VersionStatus::Outdated {
                    current: clean_req.to_string(),
                    latest: latest_version_str.to_string(),
                    newer_versions_count: newer_count.max(1),
                };
            } else if &current_v == latest_v {
                return VersionStatus::UpToDate {
                    current: clean_req.to_string(),
                    latest: latest_version_str.to_string(),
                };
            } else {
                return VersionStatus::PreRelease {
                    current: clean_req.to_string(),
                    latest_stable: latest_version_str.to_string(),
                };
            }
        }

        // Shorthand like "1" or "1.0"
        if clean_req == format!("{}", latest_v.major)
            || clean_req == format!("{}.{}", latest_v.major, latest_v.minor)
        {
            return VersionStatus::UpToDate {
                current: clean_req.to_string(),
                latest: latest_version_str.to_string(),
            };
        }

        // Otherwise, it's outdated compared to latest
        let newer_count = versions.iter().filter(|v| !v.yanked).count();

        VersionStatus::Outdated {
            current: clean_req.to_string(),
            latest: latest_version_str.to_string(),
            newer_versions_count: newer_count.max(1),
        }
    }
}

fn urlencoding_query(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' => out.push(b as char),
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sparse_index_paths() {
        assert_eq!(CratesClient::sparse_index_path("a"), "1/a");
        assert_eq!(CratesClient::sparse_index_path("ab"), "2/ab");
        assert_eq!(CratesClient::sparse_index_path("abc"), "3/a/abc");
        assert_eq!(CratesClient::sparse_index_path("serde"), "se/rd/serde");
        assert_eq!(CratesClient::sparse_index_path("tokio"), "to/ki/tokio");
    }

    #[test]
    fn test_version_status() {
        let v1 = CrateVersionInfo {
            version: "1.0.229".to_string(),
            parsed: Version::parse("1.0.229").ok(),
            yanked: false,
            pubtime: None,
            is_prerelease: false,
        };
        let v2 = CrateVersionInfo {
            version: "1.0.228".to_string(),
            parsed: Version::parse("1.0.228").ok(),
            yanked: false,
            pubtime: None,
            is_prerelease: false,
        };
        let v3 = CrateVersionInfo {
            version: "1.0.100".to_string(),
            parsed: Version::parse("1.0.100").ok(),
            yanked: false,
            pubtime: None,
            is_prerelease: false,
        };

        let versions = vec![v1, v2, v3];

        // Exact match latest
        match CratesClient::check_version_status("1.0.229", &versions) {
            VersionStatus::UpToDate { .. } => {}
            other => panic!("expected UpToDate, got {:?}", other),
        }

        // Outdated
        match CratesClient::check_version_status("1.0.100", &versions) {
            VersionStatus::Outdated { current, latest, .. } => {
                assert_eq!(current, "1.0.100");
                assert_eq!(latest, "1.0.229");
            }
            other => panic!("expected Outdated, got {:?}", other),
        }
    }
}

