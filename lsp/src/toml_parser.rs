use tower_lsp::lsp_types::{Position, Range};

pub const RESERVED_KEYS: &[&str] = &[
    "version",
    "features",
    "default-features",
    "default_features",
    "optional",
    "path",
    "git",
    "branch",
    "tag",
    "rev",
    "package",
    "workspace",
    "registry",
];

#[derive(Debug, Clone)]
pub struct LineIndex {
    line_offsets: Vec<usize>,
}

impl LineIndex {
    pub fn new(text: &str) -> Self {
        let mut line_offsets = vec![0];
        for (i, b) in text.bytes().enumerate() {
            if b == b'\n' {
                line_offsets.push(i + 1);
            }
        }
        Self { line_offsets }
    }

    pub fn offset_to_position(&self, offset: usize, text: &str) -> Position {
        let offset = offset.min(text.len());
        let line = match self.line_offsets.binary_search(&offset) {
            Ok(idx) => idx,
            Err(idx) => idx.saturating_sub(1),
        };
        let line_start = self.line_offsets[line];
        let col = text[line_start..offset].chars().count();
        Position {
            line: line as u32,
            character: col as u32,
        }
    }

    pub fn position_to_offset(&self, pos: &Position, text: &str) -> usize {
        let line = pos.line as usize;
        if line >= self.line_offsets.len() {
            return text.len();
        }
        let line_start = self.line_offsets[line];
        let line_end = self.line_offsets.get(line + 1).copied().unwrap_or(text.len());
        let line_slice = &text[line_start..line_end];
        let mut col_offset = 0;
        for (c_idx, (b_idx, _)) in line_slice.char_indices().enumerate() {
            if c_idx == pos.character as usize {
                col_offset = b_idx;
                break;
            }
            col_offset = line_slice.len();
        }
        (line_start + col_offset).min(text.len())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedDependency {
    pub crate_name: String,
    pub alias_name: String,
    pub table_name: String,
    pub version: Option<String>,
    pub is_workspace: bool,
    pub is_path: bool,
    pub is_git: bool,
    pub name_range: Range,
    pub version_range: Option<Range>,
    pub version_val_range: Option<Range>, // strictly inside the quotes
    pub hint_position: Position,          // where inline hint (✓ or ⭡) should be rendered
    pub full_range: Range,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CursorContext {
    InVersionString {
        crate_name: String,
        prefix: String,
        replace_range: Range,
    },
    InCrateKey {
        prefix: String,
        replace_range: Range,
    },
    InDependencySection {
        replace_range: Range,
    },
    None,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SectionType {
    DependencyList(String),
    SingleDependency {
        table_name: String,
        crate_key: String,
    },
    Other,
}

pub struct CargoTomlParser;

impl CargoTomlParser {
    /// Classify a section header (e.g. `dependencies`, `dependencies.sea-orm-migration`, `package`)
    pub fn classify_section(raw_header: &str) -> SectionType {
        let header = raw_header.trim();
        if header == "dependencies"
            || header == "dev-dependencies"
            || header == "build-dependencies"
            || header == "workspace.dependencies"
        {
            return SectionType::DependencyList(header.to_string());
        }

        for prefix in &[
            "dependencies.",
            "dev-dependencies.",
            "build-dependencies.",
            "workspace.dependencies.",
        ] {
            if let Some(rest) = header.strip_prefix(prefix) {
                let crate_key = rest.trim_matches(|c| c == '"' || c == '\'').trim();
                if !crate_key.is_empty() {
                    return SectionType::SingleDependency {
                        table_name: header.to_string(),
                        crate_key: crate_key.to_string(),
                    };
                }
            }
        }

        if let Some(rest) = header.strip_prefix("target.") {
            for mid in &[".dependencies", ".dev-dependencies", ".build-dependencies"] {
                if let Some(idx) = rest.find(mid) {
                    let after = &rest[idx + mid.len()..];
                    if after.is_empty() {
                        return SectionType::DependencyList(header.to_string());
                    } else if let Some(crate_part) = after.strip_prefix('.') {
                        let crate_key = crate_part.trim_matches(|c| c == '"' || c == '\'').trim();
                        if !crate_key.is_empty() {
                            return SectionType::SingleDependency {
                                table_name: header.to_string(),
                                crate_key: crate_key.to_string(),
                            };
                        }
                    }
                }
            }
        }

        SectionType::Other
    }

    #[allow(dead_code)]
    pub fn is_dependency_section(header: &str) -> bool {
        let trimmed = header.trim();
        let stripped = trimmed.trim_start_matches('[').trim_end_matches(']').trim();
        !matches!(Self::classify_section(stripped), SectionType::Other)
    }

    /// Extract all dependencies from the document across all orientations:
    /// 1. Simple string: `serde = "1.0.100"`
    /// 2. Single-line inline table: `tokio = { version = "1.0.0", features = ["full"] }`
    /// 3. Multiline inline table: `sea-orm-migration = {\n version = "2.0.3",\n features = [...] \n}`
    /// 4. TOML table block: `[dependencies.sea-orm-migration]\nversion = "2.0.3"`
    pub fn parse_dependencies(text: &str) -> Vec<ParsedDependency> {
        let line_index = LineIndex::new(text);
        let lines: Vec<&str> = text.lines().collect();
        let mut deps = Vec::new();

        let mut current_sec_type = SectionType::Other;

        // State for SingleDependency table block ([dependencies.foo])
        struct PendingSingleDep {
            table_name: String,
            crate_key: String,
            header_line: u32,
            header_name_range: Range,
            version: Option<String>,
            version_range: Option<Range>,
            version_val_range: Option<Range>,
            hint_position: Option<Position>,
            package_name: Option<String>,
            is_workspace: bool,
            is_path: bool,
            is_git: bool,
            last_line: u32,
        }
        let mut pending_single: Option<PendingSingleDep> = None;

        // State for multiline inline table in DependencyList (`foo = {\n ... \n}`)
        struct ActiveInlineTable {
            crate_key: String,
            start_line: u32,
            name_range: Range,
            brace_depth: i32,
            version: Option<String>,
            version_range: Option<Range>,
            version_val_range: Option<Range>,
            hint_position: Option<Position>,
            package_name: Option<String>,
            is_workspace: bool,
            is_path: bool,
            is_git: bool,
        }
        let mut active_inline: Option<ActiveInlineTable> = None;

        let flush_pending_single = |p: PendingSingleDep, deps: &mut Vec<ParsedDependency>, lines: &[&str]| {
            let crate_name = p.package_name.unwrap_or_else(|| p.crate_key.clone());
            let full_end_line = p.last_line.min((lines.len().saturating_sub(1)) as u32);
            let end_char = lines.get(full_end_line as usize).map(|l| l.chars().count() as u32).unwrap_or(0);
            let full_range = Range {
                start: Position { line: p.header_line, character: 0 },
                end: Position { line: full_end_line, character: end_char },
            };
            let hint_position = p.hint_position.unwrap_or(p.version_range.map(|r| r.end).unwrap_or(full_range.end));

            deps.push(ParsedDependency {
                crate_name,
                alias_name: p.crate_key,
                table_name: p.table_name,
                version: p.version,
                is_workspace: p.is_workspace,
                is_path: p.is_path,
                is_git: p.is_git,
                name_range: p.header_name_range,
                version_range: p.version_range,
                version_val_range: p.version_val_range,
                hint_position,
                full_range,
            });
        };

        let flush_active_inline = |a: ActiveInlineTable, end_line: u32, table_name: &str, deps: &mut Vec<ParsedDependency>, lines: &[&str]| {
            let crate_name = a.package_name.unwrap_or_else(|| a.crate_key.clone());
            let end_char = lines.get(end_line as usize).map(|l| l.chars().count() as u32).unwrap_or(0);
            let full_range = Range {
                start: Position { line: a.start_line, character: 0 },
                end: Position { line: end_line, character: end_char },
            };
            let hint_position = a.hint_position.unwrap_or(a.version_range.map(|r| r.end).unwrap_or(full_range.end));

            deps.push(ParsedDependency {
                crate_name,
                alias_name: a.crate_key,
                table_name: table_name.to_string(),
                version: a.version,
                is_workspace: a.is_workspace,
                is_path: a.is_path,
                is_git: a.is_git,
                name_range: a.name_range,
                version_range: a.version_range,
                version_val_range: a.version_val_range,
                hint_position,
                full_range,
            });
        };

        for (line_idx, line) in lines.iter().enumerate() {
            let line_num = line_idx as u32;
            let trimmed = line.trim();

            // Ignore full comment lines
            if trimmed.starts_with('#') {
                continue;
            }

            // Check if section header
            if trimmed.starts_with('[') && !trimmed.starts_with("[[") {
                if let Some(close_bracket) = trimmed.find(']') {
                    let sec_content = trimmed[1..close_bracket].trim();

                    // Flush any pending single dependency table block
                    if let Some(p) = pending_single.take() {
                        flush_pending_single(p, &mut deps, &lines);
                    }
                    // Flush any pending active inline table
                    if let Some(a) = active_inline.take() {
                        let sec_tbl = match &current_sec_type {
                            SectionType::DependencyList(t) => t.as_str(),
                            _ => "dependencies",
                        };
                        flush_active_inline(a, line_num.saturating_sub(1), sec_tbl, &mut deps, &lines);
                    }

                    current_sec_type = Self::classify_section(sec_content);

                    if let SectionType::SingleDependency { table_name, crate_key } = &current_sec_type {
                        // Find position of crate_key in header line
                        let header_name_range = Self::find_key_range_in_line(line, crate_key, line_num, &line_index, text);
                        pending_single = Some(PendingSingleDep {
                            table_name: table_name.clone(),
                            crate_key: crate_key.clone(),
                            header_line: line_num,
                            header_name_range,
                            version: None,
                            version_range: None,
                            version_val_range: None,
                            hint_position: None,
                            package_name: None,
                            is_workspace: false,
                            is_path: false,
                            is_git: false,
                            last_line: line_num,
                        });
                    }
                    continue;
                }
            }

            // Inside SingleDependency table block (e.g. `[dependencies.sea-orm-migration]`)
            if let Some(p) = pending_single.as_mut() {
                if !trimmed.is_empty() {
                    p.last_line = line_num;
                }

                if let Some((ver, vr, vvr, hp)) = Self::extract_version_from_line(line, line_num, &line_index, text) {
                    p.version = Some(ver);
                    p.version_range = Some(vr);
                    p.version_val_range = Some(vvr);
                    p.hint_position = Some(hp);
                }

                if let Some(pkg) = Self::extract_package_from_line(line) {
                    p.package_name = Some(pkg);
                }

                if line.contains("workspace") && line.contains("true") {
                    p.is_workspace = true;
                }
                if line.contains("path") && line.contains('=') {
                    p.is_path = true;
                }
                if line.contains("git") && line.contains('=') {
                    p.is_git = true;
                }
                continue;
            }

            // Inside DependencyList (e.g. `[dependencies]`, `[workspace.dependencies]`)
            if let SectionType::DependencyList(table_name) = &current_sec_type {
                if let Some(mut active) = active_inline.take() {
                    let delta = Self::count_brace_delta(line);
                    active.brace_depth += delta;

                    if active.version.is_none() {
                        if let Some((ver, vr, vvr, hp)) = Self::extract_version_from_line(line, line_num, &line_index, text) {
                            active.version = Some(ver);
                            active.version_range = Some(vr);
                            active.version_val_range = Some(vvr);
                            active.hint_position = Some(hp);
                        }
                    }

                    if active.package_name.is_none() {
                        if let Some(pkg) = Self::extract_package_from_line(line) {
                            active.package_name = Some(pkg);
                        }
                    }

                    if line.contains("workspace") && line.contains("true") {
                        active.is_workspace = true;
                    }
                    if line.contains("path") && line.contains('=') {
                        active.is_path = true;
                    }
                    if line.contains("git") && line.contains('=') {
                        active.is_git = true;
                    }

                    if active.brace_depth <= 0 {
                        // Closed inline table
                        flush_active_inline(active, line_num, table_name, &mut deps, &lines);
                    } else {
                        active_inline = Some(active);
                    }
                    continue;
                }

                // Not in multiline inline table: check if line declares a dependency
                let trimmed_line = line.trim_start();
                if trimmed_line.is_empty() || trimmed_line.starts_with('#') {
                    continue;
                }

                let Some(eq_idx) = line.find('=') else {
                    continue;
                };

                let raw_key = line[..eq_idx].trim();
                if raw_key.is_empty() {
                    continue;
                }

                let key_clean = raw_key.trim_matches(|c| c == '"' || c == '\'');
                // Never treat reserved Cargo property keys as crate names!
                if RESERVED_KEYS.contains(&key_clean) {
                    continue;
                }

                let name_range = Self::find_key_range_in_line(line, raw_key, line_num, &line_index, text);
                let raw_val = &line[eq_idx + 1..];
                let brace_delta = Self::count_brace_delta(raw_val);

                if raw_val.contains('{') && brace_delta > 0 {
                    // Starts a multiline inline table!
                    let (ver_opt, vr_opt, vvr_opt, hp_opt) = match Self::extract_version_from_line(line, line_num, &line_index, text) {
                        Some((v, vr, vvr, hp)) => (Some(v), Some(vr), Some(vvr), Some(hp)),
                        None => (None, None, None, None),
                    };
                    let pkg_opt = Self::extract_package_from_line(line);
                    let is_workspace = raw_val.contains("workspace") && raw_val.contains("true");
                    let is_path = raw_val.contains("path");
                    let is_git = raw_val.contains("git");

                    active_inline = Some(ActiveInlineTable {
                        crate_key: key_clean.to_string(),
                        start_line: line_num,
                        name_range,
                        brace_depth: brace_delta,
                        version: ver_opt,
                        version_range: vr_opt,
                        version_val_range: vvr_opt,
                        hint_position: hp_opt,
                        package_name: pkg_opt,
                        is_workspace,
                        is_path,
                        is_git,
                    });
                } else if raw_val.contains('{') {
                    // Single-line inline table
                    let (ver_opt, vr_opt, vvr_opt, hp_opt) = match Self::extract_version_from_line(line, line_num, &line_index, text) {
                        Some((v, vr, vvr, hp)) => (Some(v), Some(vr), Some(vvr), Some(hp)),
                        None => (None, None, None, None),
                    };
                    let pkg_opt = Self::extract_package_from_line(line);
                    let is_workspace = raw_val.contains("workspace") && raw_val.contains("true");
                    let is_path = raw_val.contains("path");
                    let is_git = raw_val.contains("git");
                    let end_char = line.chars().count() as u32;

                    let full_range = Range {
                        start: Position { line: line_num, character: 0 },
                        end: Position { line: line_num, character: end_char },
                    };
                    let hint_position = hp_opt.unwrap_or(vr_opt.map(|r| r.end).unwrap_or(full_range.end));

                    deps.push(ParsedDependency {
                        crate_name: pkg_opt.unwrap_or_else(|| key_clean.to_string()),
                        alias_name: key_clean.to_string(),
                        table_name: table_name.clone(),
                        version: ver_opt,
                        is_workspace,
                        is_path,
                        is_git,
                        name_range,
                        version_range: vr_opt,
                        version_val_range: vvr_opt,
                        hint_position,
                        full_range,
                    });
                } else {
                    // Simple string: serde = "1.0.100"
                    let (ver_opt, vr_opt, vvr_opt, hp_opt) = match Self::extract_simple_string_version(line, eq_idx, line_num, &line_index, text) {
                        Some((v, vr, vvr, hp)) => (Some(v), Some(vr), Some(vvr), Some(hp)),
                        None => (None, None, None, None),
                    };
                    let end_char = line.chars().count() as u32;
                    let full_range = Range {
                        start: Position { line: line_num, character: 0 },
                        end: Position { line: line_num, character: end_char },
                    };
                    let hint_position = hp_opt.unwrap_or(vr_opt.map(|r| r.end).unwrap_or(full_range.end));

                    deps.push(ParsedDependency {
                        crate_name: key_clean.to_string(),
                        alias_name: key_clean.to_string(),
                        table_name: table_name.clone(),
                        version: ver_opt,
                        is_workspace: false,
                        is_path: false,
                        is_git: false,
                        name_range,
                        version_range: vr_opt,
                        version_val_range: vvr_opt,
                        hint_position,
                        full_range,
                    });
                }
            }
        }

        // Flush trailing pending single dependency
        if let Some(p) = pending_single {
            flush_pending_single(p, &mut deps, &lines);
        }
        // Flush trailing active inline table
        if let Some(a) = active_inline {
            let sec_tbl = match &current_sec_type {
                SectionType::DependencyList(t) => t.as_str(),
                _ => "dependencies",
            };
            flush_active_inline(a, lines.len().saturating_sub(1) as u32, sec_tbl, &mut deps, &lines);
        }

        deps
    }

    /// Find dependency at or containing cursor position
    pub fn find_dependency_at(deps: &[ParsedDependency], pos: &Position) -> Option<ParsedDependency> {
        for dep in deps {
            if pos.line >= dep.full_range.start.line && pos.line <= dep.full_range.end.line {
                return Some(dep.clone());
            }
        }
        None
    }

    /// Determine cursor context for autocompletion
    pub fn get_cursor_context(text: &str, pos: &Position) -> CursorContext {
        let lines: Vec<&str> = text.lines().collect();
        let line_idx = pos.line as usize;
        if line_idx >= lines.len() {
            return CursorContext::None;
        }

        let current_line = lines[line_idx];
        let col = (pos.character as usize).min(current_line.len());
        let before_cursor = &current_line[..col];

        let deps = Self::parse_dependencies(text);

        // 1. If cursor is on or inside an existing dependency:
        if let Some(dep) = Self::find_dependency_at(&deps, pos) {
            // Check if cursor is inside version string quotes:
            if let Some(val_range) = &dep.version_val_range {
                if pos.line == val_range.start.line
                    && pos.character >= val_range.start.character
                    && pos.character <= val_range.end.character + 1
                {
                    let prefix = if let Some(v) = &dep.version {
                        let offset_in_v = (pos.character.saturating_sub(val_range.start.character)) as usize;
                        if offset_in_v <= v.len() {
                            v[..offset_in_v].to_string()
                        } else {
                            v.clone()
                        }
                    } else {
                        "".to_string()
                    };

                    return CursorContext::InVersionString {
                        crate_name: dep.crate_name,
                        prefix,
                        replace_range: *val_range,
                    };
                }
            }

            // Check if cursor is after an open quote on current line
            if let Some(eq_pos) = before_cursor.find('=') {
                let after_eq = &before_cursor[eq_pos + 1..];
                if let Some(quote_idx) = after_eq.rfind('"').or_else(|| after_eq.rfind('\'')) {
                    let prefix = after_eq[quote_idx + 1..].to_string();
                    let start_char = (eq_pos + 1 + quote_idx + 1) as u32;
                    return CursorContext::InVersionString {
                        crate_name: dep.crate_name,
                        prefix,
                        replace_range: Range {
                            start: Position { line: pos.line, character: start_char },
                            end: *pos,
                        },
                    };
                }
            }
        }

        // Determine current section
        let mut current_sec_type = SectionType::Other;
        for (idx, line) in lines.iter().enumerate() {
            if idx > line_idx {
                break;
            }
            let trimmed = line.trim();
            if trimmed.starts_with('[') && !trimmed.starts_with("[[") {
                if let Some(close_bracket) = trimmed.find(']') {
                    let sec_content = trimmed[1..close_bracket].trim();
                    current_sec_type = Self::classify_section(sec_content);
                }
            }
        }

        match current_sec_type {
            SectionType::DependencyList(_) => {
                // If before cursor contains '=', and quotes opened
                if let Some(eq_pos) = before_cursor.find('=') {
                    let raw_key = before_cursor[..eq_pos].trim();
                    let key_clean = raw_key.trim_matches(|c| c == '"' || c == '\'');
                    let after_eq = &before_cursor[eq_pos + 1..];

                    if let Some(quote_idx) = after_eq.rfind('"').or_else(|| after_eq.rfind('\'')) {
                        let prefix = after_eq[quote_idx + 1..].to_string();
                        let start_char = (eq_pos + 1 + quote_idx + 1) as u32;
                        return CursorContext::InVersionString {
                            crate_name: key_clean.to_string(),
                            prefix,
                            replace_range: Range {
                                start: Position { line: pos.line, character: start_char },
                                end: *pos,
                            },
                        };
                    }
                }

                // If before cursor has no '=', user is typing crate key
                if !before_cursor.contains('=') {
                    let trimmed_before = before_cursor.trim_start();
                    let leading_spaces = before_cursor.len() - trimmed_before.len();
                    let prefix = trimmed_before.trim_end();

                    return CursorContext::InCrateKey {
                        prefix: prefix.to_string(),
                        replace_range: Range {
                            start: Position { line: pos.line, character: leading_spaces as u32 },
                            end: *pos,
                        },
                    };
                }

                CursorContext::InDependencySection {
                    replace_range: Range {
                        start: *pos,
                        end: *pos,
                    },
                }
            }
            SectionType::SingleDependency { crate_key, .. } => {
                // In single dependency section: if typing after '=' and '"'
                if let Some(eq_pos) = before_cursor.find('=') {
                    let after_eq = &before_cursor[eq_pos + 1..];
                    if let Some(quote_idx) = after_eq.rfind('"').or_else(|| after_eq.rfind('\'')) {
                        let prefix = after_eq[quote_idx + 1..].to_string();
                        let start_char = (eq_pos + 1 + quote_idx + 1) as u32;
                        return CursorContext::InVersionString {
                            crate_name: crate_key,
                            prefix,
                            replace_range: Range {
                                start: Position { line: pos.line, character: start_char },
                                end: *pos,
                            },
                        };
                    }
                }
                CursorContext::None
            }
            SectionType::Other => CursorContext::None,
        }
    }

    fn find_key_range_in_line(
        line: &str,
        key: &str,
        line_num: u32,
        line_index: &LineIndex,
        full_text: &str,
    ) -> Range {
        let line_offset = line_index.position_to_offset(&Position { line: line_num, character: 0 }, full_text);
        let key_start = line.find(key).unwrap_or(0);
        let key_end = key_start + key.len();

        Range {
            start: line_index.offset_to_position(line_offset + key_start, full_text),
            end: line_index.offset_to_position(line_offset + key_end, full_text),
        }
    }

    fn find_quoted_string(slice: &str) -> Option<(&str, usize, usize)> {
        let start_quote = slice.find(|c| c == '"' || c == '\'')?;
        let quote_char = slice.as_bytes()[start_quote];
        let rest = &slice[start_quote + 1..];
        let end_rel = rest.find(quote_char as char)?;
        let end_quote = start_quote + 1 + end_rel;
        Some((&slice[start_quote + 1..end_quote], start_quote, end_quote))
    }

    fn extract_version_from_line(
        line: &str,
        line_num: u32,
        line_index: &LineIndex,
        full_text: &str,
    ) -> Option<(String, Range, Range, Position)> {
        let ver_idx = line.find("version")?;
        let before_ver = &line[..ver_idx];
        if let Some(prev) = before_ver.chars().last() {
            if prev.is_alphanumeric() || prev == '_' || prev == '-' {
                return None;
            }
        }
        let after_ver = &line[ver_idx + 7..];
        let eq_rel = after_ver.find('=')?;
        let after_eq = &after_ver[eq_rel + 1..];
        let (ver_str, q_start_rel, q_end_rel) = Self::find_quoted_string(after_eq)?;

        let line_start_offset = line_index.position_to_offset(&Position { line: line_num, character: 0 }, full_text);
        let q_start_in_line = ver_idx + 7 + eq_rel + 1 + q_start_rel;
        let q_end_in_line = ver_idx + 7 + eq_rel + 1 + q_end_rel + 1;

        let version_range = Range {
            start: line_index.offset_to_position(line_start_offset + q_start_in_line, full_text),
            end: line_index.offset_to_position(line_start_offset + q_end_in_line, full_text),
        };
        let version_val_range = Range {
            start: line_index.offset_to_position(line_start_offset + q_start_in_line + 1, full_text),
            end: line_index.offset_to_position(line_start_offset + q_end_in_line - 1, full_text),
        };
        let hint_position = version_range.end;

        Some((ver_str.to_string(), version_range, version_val_range, hint_position))
    }

    fn extract_simple_string_version(
        line: &str,
        eq_idx: usize,
        line_num: u32,
        line_index: &LineIndex,
        full_text: &str,
    ) -> Option<(String, Range, Range, Position)> {
        let after_eq = &line[eq_idx + 1..];
        let (ver_str, q_start_rel, q_end_rel) = Self::find_quoted_string(after_eq)?;

        let line_start_offset = line_index.position_to_offset(&Position { line: line_num, character: 0 }, full_text);
        let q_start_in_line = eq_idx + 1 + q_start_rel;
        let q_end_in_line = eq_idx + 1 + q_end_rel + 1;

        let version_range = Range {
            start: line_index.offset_to_position(line_start_offset + q_start_in_line, full_text),
            end: line_index.offset_to_position(line_start_offset + q_end_in_line, full_text),
        };
        let version_val_range = Range {
            start: line_index.offset_to_position(line_start_offset + q_start_in_line + 1, full_text),
            end: line_index.offset_to_position(line_start_offset + q_end_in_line - 1, full_text),
        };
        let hint_position = version_range.end;

        Some((ver_str.to_string(), version_range, version_val_range, hint_position))
    }

    fn extract_package_from_line(line: &str) -> Option<String> {
        let pkg_idx = line.find("package")?;
        let before_pkg = &line[..pkg_idx];
        if let Some(prev) = before_pkg.chars().last() {
            if prev.is_alphanumeric() || prev == '_' || prev == '-' {
                return None;
            }
        }
        let after_pkg = &line[pkg_idx + 7..];
        let eq_rel = after_pkg.find('=')?;
        let after_eq = &after_pkg[eq_rel + 1..];
        let (pkg_str, _, _) = Self::find_quoted_string(after_eq)?;
        Some(pkg_str.to_string())
    }

    fn count_brace_delta(line: &str) -> i32 {
        let mut delta = 0;
        let mut in_quote = None;
        let mut chars = line.chars().peekable();

        while let Some(c) = chars.next() {
            if c == '\\' {
                chars.next();
                continue;
            }
            if let Some(q) = in_quote {
                if c == q {
                    in_quote = None;
                }
            } else if c == '"' || c == '\'' {
                in_quote = Some(c);
            } else if c == '#' {
                break;
            } else if c == '{' {
                delta += 1;
            } else if c == '}' {
                delta -= 1;
            }
        }
        delta
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_dependencies_all_orientations() {
        let toml = r#"
[package]
name = "my-app"
version = "0.1.0"

[dependencies]
serde = "1.0.100"
tokio = { version = "1.0.0", features = ["full"] }

sea-orm-migration = { version = "2.0.3", features = [
    "runtime-tokio-rustls",
    "sqlx-postgres"
]
}

sea-orm-migration-multiline = {
    version = "2.0.3",
    features = [
        "sqlx-postgres"
    ]
}

local-lib = { path = "../local" }
workspace-dep = { workspace = true }

[dependencies.sea-orm-migration-table]
version = "2.0.3"
features = ["sqlx-postgres"]

[target.'cfg(unix)'.dependencies.sea-orm-migration-target]
version = "2.0.3"

[dev-dependencies]
tempfile = "3.2.0"
"#;

        let deps = CargoTomlParser::parse_dependencies(toml);

        // Ensure crate names are never property names like "version" or "features"
        for dep in &deps {
            assert_ne!(dep.crate_name, "version", "Found dep erroneously named 'version'!");
            assert_ne!(dep.crate_name, "features", "Found dep erroneously named 'features'!");
            assert_ne!(dep.crate_name, "my-app", "Package should not be parsed as dep");
        }

        let serde_dep = deps.iter().find(|d| d.crate_name == "serde").unwrap();
        assert_eq!(serde_dep.version.as_deref(), Some("1.0.100"));

        let tokio_dep = deps.iter().find(|d| d.crate_name == "tokio").unwrap();
        assert_eq!(tokio_dep.version.as_deref(), Some("1.0.0"));

        let sea_dep1 = deps.iter().find(|d| d.crate_name == "sea-orm-migration").unwrap();
        assert_eq!(sea_dep1.version.as_deref(), Some("2.0.3"));
        // hint_position must be on the version line (line 9), not at end of the features array
        assert_eq!(sea_dep1.hint_position.line, 9);

        let sea_dep2 = deps.iter().find(|d| d.crate_name == "sea-orm-migration-multiline").unwrap();
        assert_eq!(sea_dep2.version.as_deref(), Some("2.0.3"));
        assert_eq!(sea_dep2.hint_position.line, 16);

        let sea_dep3 = deps.iter().find(|d| d.crate_name == "sea-orm-migration-table").unwrap();
        assert_eq!(sea_dep3.version.as_deref(), Some("2.0.3"));
        assert_eq!(sea_dep3.hint_position.line, 26);

        let sea_dep4 = deps.iter().find(|d| d.crate_name == "sea-orm-migration-target").unwrap();
        assert_eq!(sea_dep4.version.as_deref(), Some("2.0.3"));
        assert_eq!(sea_dep4.hint_position.line, 30);

        let tempfile_dep = deps.iter().find(|d| d.crate_name == "tempfile").unwrap();
        assert_eq!(tempfile_dep.version.as_deref(), Some("3.2.0"));
    }

    #[test]
    fn test_find_dependency_at_multiline() {
        let toml = r#"
[dependencies]
sea-orm-migration = {
    version = "2.0.3",
    features = ["full"]
}
"#;
        let deps = CargoTomlParser::parse_dependencies(toml);
        assert_eq!(deps.len(), 1);

        // Hover on line 3 (version = "2.0.3")
        let pos_ver = Position { line: 3, character: 8 };
        let dep = CargoTomlParser::find_dependency_at(&deps, &pos_ver).unwrap();
        assert_eq!(dep.crate_name, "sea-orm-migration");
        assert_ne!(dep.crate_name, "version");

        // Hover on line 4 (features = ["full"])
        let pos_feat = Position { line: 4, character: 6 };
        let dep = CargoTomlParser::find_dependency_at(&deps, &pos_feat).unwrap();
        assert_eq!(dep.crate_name, "sea-orm-migration");
    }

    #[test]
    fn test_find_dependency_at_table_block() {
        let toml = r#"
[dependencies.sea-orm-migration]
version = "2.0.3"
features = ["full"]
"#;
        let deps = CargoTomlParser::parse_dependencies(toml);
        assert_eq!(deps.len(), 1);

        // Hover on header line 1
        let pos_header = Position { line: 1, character: 16 };
        let dep1 = CargoTomlParser::find_dependency_at(&deps, &pos_header).unwrap();
        assert_eq!(dep1.crate_name, "sea-orm-migration");

        // Hover on version line 2
        let pos_ver = Position { line: 2, character: 5 };
        let dep2 = CargoTomlParser::find_dependency_at(&deps, &pos_ver).unwrap();
        assert_eq!(dep2.crate_name, "sea-orm-migration");
        assert_ne!(dep2.crate_name, "version");
    }

    #[test]
    fn test_cursor_context_version() {
        let toml = "[dependencies]\nserde = \"1.\"\n";
        let pos = Position { line: 1, character: 11 };
        let ctx = CargoTomlParser::get_cursor_context(toml, &pos);
        match ctx {
            CursorContext::InVersionString { crate_name, prefix, .. } => {
                assert_eq!(crate_name, "serde");
                assert_eq!(prefix, "1.");
            }
            other => panic!("expected InVersionString, got {:?}", other),
        }
    }

    #[test]
    fn test_cursor_context_table_block() {
        let toml = "[dependencies.sea-orm-migration]\nversion = \"2.\"\n";
        let pos = Position { line: 1, character: 13 };
        let ctx = CargoTomlParser::get_cursor_context(toml, &pos);
        match ctx {
            CursorContext::InVersionString { crate_name, prefix, .. } => {
                assert_eq!(crate_name, "sea-orm-migration");
                assert_eq!(prefix, "2.");
            }
            other => panic!("expected InVersionString, got {:?}", other),
        }
    }

    #[test]
    fn test_cursor_context_crate_name() {
        let toml = "[dependencies]\ntok\n";
        let pos = Position { line: 1, character: 3 };
        let ctx = CargoTomlParser::get_cursor_context(toml, &pos);
        match ctx {
            CursorContext::InCrateKey { prefix, .. } => {
                assert_eq!(prefix, "tok");
            }
            other => panic!("expected InCrateKey, got {:?}", other),
        }
    }
}
