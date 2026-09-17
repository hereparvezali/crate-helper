use std::collections::HashMap;
use std::sync::Arc;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::RwLock;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer};

use crate::crates_client::{CratesClient, VersionStatus};
use crate::toml_parser::{CargoTomlParser, CursorContext};

pub struct Backend {
    pub client: Client,
    pub crates: CratesClient,
    pub documents: Arc<RwLock<HashMap<Url, String>>>,
    pub http_port: u16,
}

impl Backend {
    pub fn new(client: Client, http_port: u16) -> Self {
        Self {
            client,
            crates: CratesClient::new(),
            documents: Arc::new(RwLock::new(HashMap::new())),
            http_port,
        }
    }

    async fn update_diagnostics(&self, uri: Url, _text: &str) {
        // Do not emit warning diagnostics to keep the editor clean and squiggly-free.
        self.client.publish_diagnostics(uri, Vec::new(), None).await;
        // Refresh inline hints
        let _ = self.client.inlay_hint_refresh().await;
    }
}

pub fn spawn_http_replacer(
    listener: TcpListener,
    client: Client,
    documents: Arc<RwLock<HashMap<Url, String>>>,
) {
    tokio::spawn(async move {
        loop {
            let (mut socket, _) = match listener.accept().await {
                Ok(s) => s,
                Err(_) => break,
            };

            let client = client.clone();
            let documents = documents.clone();

            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                let n = match socket.read(&mut buf).await {
                    Ok(n) if n > 0 => n,
                    _ => return,
                };

                let req_str = String::from_utf8_lossy(&buf[..n]);
                let first_line = req_str.lines().next().unwrap_or("");
                if let Some(query_start) = first_line.find("/replace?") {
                    let query_end = first_line[query_start..]
                        .find(' ')
                        .unwrap_or(first_line.len() - query_start);
                    let query = &first_line[query_start + 9..query_start + query_end];

                    let mut uri_opt: Option<Url> = None;
                    let mut crate_opt: Option<String> = None;
                    let mut version_opt: Option<String> = None;

                    for pair in query.split('&') {
                        if let Some((k, v)) = pair.split_once('=') {
                            let decoded = urlencoding::decode(v).unwrap_or_default().to_string();
                            match k {
                                "uri" => uri_opt = Url::parse(&decoded).ok(),
                                "crate" => crate_opt = Some(decoded),
                                "version" => version_opt = Some(decoded),
                                _ => {}
                            }
                        }
                    }

                    if let (Some(uri), Some(crate_name), Some(new_version)) =
                        (uri_opt, crate_opt, version_opt)
                    {
                        let doc_text = {
                            let docs = documents.read().await;
                            docs.get(&uri).cloned()
                        };

                        if let Some(text) = doc_text {
                            let deps = CargoTomlParser::parse_dependencies(&text);
                            if let Some(dep) = deps
                                .iter()
                                .find(|d| d.crate_name == crate_name || d.alias_name == crate_name)
                            {
                                if let Some(val_range) = dep.version_val_range {
                                    let mut changes = HashMap::new();
                                    changes.insert(
                                        uri.clone(),
                                        vec![TextEdit {
                                            range: val_range,
                                            new_text: new_version.clone(),
                                        }],
                                    );

                                    let _ = client
                                        .apply_edit(WorkspaceEdit {
                                            changes: Some(changes),
                                            document_changes: None,
                                            change_annotations: None,
                                        })
                                        .await;
                                }
                            }
                        }

                        let html = format!(
                            "<!DOCTYPE html><html><head><meta charset=\"utf-8\"><title>Version Updated</title></head><body style=\"background:#18181b;color:#f4f4f5;font-family:system-ui,-apple-system,sans-serif;display:flex;align-items:center;justify-content:center;height:100vh;margin:0;\"><div style=\"text-align:center;padding:24px 32px;border:1px solid #3f3f46;border-radius:12px;background:#27272a;box-shadow:0 4px 16px rgba(0,0,0,0.5);\"><h2>&#10004; Updated <code>{}</code> to <code>{}</code></h2><p style=\"color:#a1a1aa;margin-top:8px;\">Applied to Cargo.toml in Zed.<br>Closing this tab...</p></div><script>setTimeout(function(){{ window.close(); }}, 400);</script></body></html>",
                            crate_name, new_version
                        );

                        let response = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            html.len(),
                            html
                        );

                        let _ = socket.write_all(response.as_bytes()).await;
                        let _ = socket.flush().await;
                        return;
                    }
                }

                let _ = socket
                    .write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                    .await;
            });
        }
    });
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                completion_provider: Some(CompletionOptions {
                    resolve_provider: Some(false),
                    trigger_characters: Some(vec![
                        "\"".to_string(),
                        "'".to_string(),
                        ".".to_string(),
                        "=".to_string(),
                        " ".to_string(),
                    ]),
                    all_commit_characters: None,
                    work_done_progress_options: Default::default(),
                    completion_item: None,
                }),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                inlay_hint_provider: Some(OneOf::Left(true)),
                code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
                ..Default::default()
            },
            server_info: Some(ServerInfo {
                name: "crate-helper-lsp".to_string(),
                version: Some("0.1.0".to_string()),
            }),
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "crate-helper LSP server initialized!")
            .await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri;
        let text = params.text_document.text;
        {
            let mut docs = self.documents.write().await;
            docs.insert(uri.clone(), text.clone());
        }
        self.update_diagnostics(uri, &text).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;
        if let Some(change) = params.content_changes.into_iter().last() {
            let text = change.text;
            {
                let mut docs = self.documents.write().await;
                docs.insert(uri.clone(), text.clone());
            }
            self.update_diagnostics(uri, &text).await;
        }
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        let uri = params.text_document.uri;
        let text_opt = {
            let docs = self.documents.read().await;
            docs.get(&uri).cloned()
        };
        if let Some(text) = text_opt {
            self.update_diagnostics(uri, &text).await;
        }
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let uri = params.text_document_position.text_document.uri;
        let pos = params.text_document_position.position;

        let doc_text = {
            let docs = self.documents.read().await;
            match docs.get(&uri) {
                Some(t) => t.clone(),
                None => return Ok(None),
            }
        };

        let context = CargoTomlParser::get_cursor_context(&doc_text, &pos);

        match context {
            CursorContext::InVersionString {
                crate_name,
                prefix,
                replace_range,
            } => {
                let mut items = Vec::new();
                if let Ok(versions) = self.crates.get_versions(&crate_name).await {
                    let mut found_latest_stable = false;

                    for (idx, ver) in versions.iter().enumerate() {
                        if !prefix.is_empty() && !ver.version.starts_with(&prefix) {
                            continue;
                        }

                        let is_latest = !found_latest_stable && !ver.yanked && !ver.is_prerelease;
                        if is_latest {
                            found_latest_stable = true;
                        }

                        let detail = if is_latest {
                            format!(
                                "latest stable{}",
                                ver.pubtime
                                    .as_ref()
                                    .map(|d| format!(" ({})", &d[..10.min(d.len())]))
                                    .unwrap_or_default()
                            )
                        } else if ver.yanked {
                            "yanked".to_string()
                        } else if ver.is_prerelease {
                            "pre-release".to_string()
                        } else if let Some(d) = &ver.pubtime {
                            format!("released {}", &d[..10.min(d.len())])
                        } else {
                            "".to_string()
                        };

                        let sort_text = format!("{:05}", idx);
                        items.push(CompletionItem {
                            label: ver.version.clone(),
                            kind: Some(CompletionItemKind::VALUE),
                            detail: Some(detail),
                            documentation: Some(Documentation::MarkupContent(MarkupContent {
                                kind: MarkupKind::Markdown,
                                value: format!(
                                    "### `{}` v{}\n{}",
                                    crate_name,
                                    ver.version,
                                    if is_latest {
                                        "**Latest stable release**\n"
                                    } else {
                                        ""
                                    }
                                ),
                            })),
                            sort_text: Some(sort_text),
                            text_edit: Some(CompletionTextEdit::Edit(TextEdit {
                                range: replace_range,
                                new_text: ver.version.clone(),
                            })),
                            ..Default::default()
                        });
                    }
                }
                Ok(Some(CompletionResponse::Array(items)))
            }

            CursorContext::InCrateKey {
                prefix,
                replace_range,
            } => {
                let search_results = self.crates.search_crates(&prefix).await;
                let mut items = Vec::new();

                for (idx, res) in search_results.into_iter().enumerate() {
                    let detail = res.description.clone().unwrap_or_default();
                    let latest_ver = if !res.max_version.is_empty() {
                        res.max_version.clone()
                    } else if let Ok(vers) = self.crates.get_versions(&res.name).await {
                        vers.iter()
                            .find(|v| !v.yanked && !v.is_prerelease)
                            .map(|v| v.version.clone())
                            .unwrap_or_else(|| "1.0".to_string())
                    } else {
                        "1.0".to_string()
                    };

                    let sort_text = format!("{:04}_{}", idx, res.name);
                    items.push(CompletionItem {
                        label: res.name.clone(),
                        kind: Some(CompletionItemKind::MODULE),
                        detail: Some(format!("v{} • {}", latest_ver, detail)),
                        documentation: res.description.as_ref().map(|d| {
                            Documentation::MarkupContent(MarkupContent {
                                kind: MarkupKind::Markdown,
                                value: format!(
                                    "### **{}** (v{})\n\n{}\n\n[crates.io](https://crates.io/crates/{})",
                                    res.name, latest_ver, d, res.name
                                ),
                            })
                        }),
                        sort_text: Some(sort_text),
                        insert_text_format: Some(InsertTextFormat::SNIPPET),
                        text_edit: Some(CompletionTextEdit::Edit(TextEdit {
                            range: replace_range,
                            new_text: format!("{} = \"{}\"", res.name, latest_ver),
                        })),
                        ..Default::default()
                    });
                }

                Ok(Some(CompletionResponse::Array(items)))
            }

            CursorContext::InDependencySection { replace_range } => {
                let search_results = self.crates.search_crates("").await;
                let mut items = Vec::new();

                for (idx, res) in search_results.into_iter().enumerate() {
                    let detail = res.description.clone().unwrap_or_default();
                    let latest_ver = if !res.max_version.is_empty() {
                        res.max_version.clone()
                    } else if let Ok(vers) = self.crates.get_versions(&res.name).await {
                        vers.iter()
                            .find(|v| !v.yanked && !v.is_prerelease)
                            .map(|v| v.version.clone())
                            .unwrap_or_else(|| "1.0".to_string())
                    } else {
                        "1.0".to_string()
                    };

                    let sort_text = format!("{:04}_{}", idx, res.name);
                    items.push(CompletionItem {
                        label: res.name.clone(),
                        kind: Some(CompletionItemKind::MODULE),
                        detail: Some(format!("v{} • {}", latest_ver, detail)),
                        documentation: res.description.as_ref().map(|d| {
                            Documentation::MarkupContent(MarkupContent {
                                kind: MarkupKind::Markdown,
                                value: format!(
                                    "### **{}** (v{})\n\n{}\n\n[crates.io](https://crates.io/crates/{})",
                                    res.name, latest_ver, d, res.name
                                ),
                            })
                        }),
                        sort_text: Some(sort_text),
                        insert_text_format: Some(InsertTextFormat::SNIPPET),
                        text_edit: Some(CompletionTextEdit::Edit(TextEdit {
                            range: replace_range,
                            new_text: format!("{} = \"{}\"", res.name, latest_ver),
                        })),
                        ..Default::default()
                    });
                }

                Ok(Some(CompletionResponse::Array(items)))
            }

            CursorContext::None => Ok(None),
        }
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;

        let doc_text = {
            let docs = self.documents.read().await;
            match docs.get(&uri) {
                Some(t) => t.clone(),
                None => return Ok(None),
            }
        };

        let line = doc_text.lines().nth(pos.line as usize).unwrap_or("");
        if line.trim().is_empty() || line.trim().starts_with('#') {
            return Ok(None);
        }

        let deps = CargoTomlParser::parse_dependencies(&doc_text);
        let dep = match CargoTomlParser::find_dependency_at(&deps, &pos) {
            Some(d) => d,
            None => return Ok(None),
        };

        let versions = self.crates.get_versions(&dep.crate_name).await.ok();

        let mut md = String::new();
        // ONLY the name should be shown as requested
        md.push_str(&format!("### {}\n\n", dep.crate_name));

        if let Some(vers) = &versions {
            for (idx, ver) in vers.iter().take(20).enumerate() {
                let is_latest = idx == 0 && !ver.yanked && !ver.is_prerelease;
                let is_current = dep.version.as_deref() == Some(&ver.version);

                // Clicking this link invokes our local HTTP replacer, which applies the edit in Zed
                let replace_url = format!(
                    "http://127.0.0.1:{}/replace?uri={}&crate={}&version={}",
                    self.http_port,
                    urlencoding::encode(uri.as_str()),
                    urlencoding::encode(&dep.crate_name),
                    urlencoding::encode(&ver.version)
                );

                let tag = if is_current {
                    " *(current)*"
                } else if is_latest {
                    " *(latest)*"
                } else if ver.yanked {
                    " *(yanked)*"
                } else if ver.is_prerelease {
                    " *(pre-release)*"
                } else {
                    ""
                };

                md.push_str(&format!("- [{}]({}){}\n", ver.version, replace_url, tag));
            }
        } else {
            md.push_str("*Could not fetch versions from crates.io*\n");
        }

        Ok(Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: md,
            }),
            range: Some(dep.full_range),
        }))
    }

    async fn inlay_hint(&self, params: InlayHintParams) -> Result<Option<Vec<InlayHint>>> {
        let uri = params.text_document.uri;
        let doc_text = {
            let docs = self.documents.read().await;
            match docs.get(&uri) {
                Some(t) => t.clone(),
                None => return Ok(None),
            }
        };

        let deps = CargoTomlParser::parse_dependencies(&doc_text);
        let mut hints = Vec::new();

        for dep in deps {
            let Some(ver_str) = &dep.version else {
                continue;
            };
            if dep.is_path || dep.is_git || dep.is_workspace {
                continue;
            }

            if let Ok(versions) = self.crates.get_versions(&dep.crate_name).await {
                let status = CratesClient::check_version_status(ver_str, &versions);
                match status {
                    VersionStatus::Outdated { current, latest, .. } => {
                        hints.push(InlayHint {
                            position: dep.hint_position,
                            label: InlayHintLabel::String(format!(" ⭡ {latest}")),
                            kind: Some(InlayHintKind::TYPE),
                            text_edits: None,
                            tooltip: Some(InlayHintTooltip::String(format!(
                                "Update available: {current} ➔ {latest}"
                            ))),
                            padding_left: Some(true),
                            padding_right: None,
                            data: None,
                        });
                    }
                    VersionStatus::UpToDate { .. } => {
                        hints.push(InlayHint {
                            position: dep.hint_position,
                            label: InlayHintLabel::String(" ✓".to_string()),
                            kind: Some(InlayHintKind::TYPE),
                            text_edits: None,
                            tooltip: Some(InlayHintTooltip::String("Up to date".to_string())),
                            padding_left: Some(true),
                            padding_right: None,
                            data: None,
                        });
                    }
                    _ => {}
                }
            }
        }

        Ok(Some(hints))
    }

    async fn code_action(&self, params: CodeActionParams) -> Result<Option<CodeActionResponse>> {
        let uri = params.text_document.uri;
        let mut actions = Vec::new();

        // All 20 versions as selectable quick actions when cursor is on a dependency
        let doc_text = {
            let docs = self.documents.read().await;
            docs.get(&uri).cloned()
        };

        if let Some(text) = doc_text {
            let deps = CargoTomlParser::parse_dependencies(&text);
            if let Some(dep) = CargoTomlParser::find_dependency_at(&deps, &params.range.start) {
                if let Some(val_range) = dep.version_val_range {
                    if let Ok(versions) = self.crates.get_versions(&dep.crate_name).await {
                        for (idx, ver) in versions.iter().take(20).enumerate() {
                            if dep.version.as_deref() == Some(&ver.version) {
                                continue;
                            }
                            let is_latest = idx == 0 && !ver.yanked && !ver.is_prerelease;
                            let title = if is_latest {
                                format!("Replace with v{} (latest)", ver.version)
                            } else {
                                format!("Replace with v{}", ver.version)
                            };

                            let mut changes = HashMap::new();
                            changes.insert(
                                uri.clone(),
                                vec![TextEdit {
                                    range: val_range,
                                    new_text: ver.version.clone(),
                                }],
                            );

                            actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                                title,
                                kind: Some(CodeActionKind::QUICKFIX),
                                diagnostics: None,
                                edit: Some(WorkspaceEdit {
                                    changes: Some(changes),
                                    document_changes: None,
                                    change_annotations: None,
                                }),
                                command: None,
                                is_preferred: Some(is_latest && actions.is_empty()),
                                disabled: None,
                                data: None,
                            }));
                        }
                    }
                }
            }
        }

        Ok(Some(actions))
    }
}
