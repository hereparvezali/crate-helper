use std::fs;
use zed_extension_api::LanguageServerId;
use zed_extension_api::{self as zed, Result};

const BINARY_NAME: &str = "crate-helper-lsp";

struct CrateHelperExtension {
    cached_binary_path: Option<String>,
}

impl CrateHelperExtension {
    fn language_server_binary_path(
        &mut self,
        _language_server_id: &LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<String> {
        if let Some(path) = &self.cached_binary_path {
            if fs::metadata(path).is_ok_and(|stat| stat.is_file()) {
                return Ok(path.clone());
            }
        }

        // 1. Check if binary is in PATH
        if let Some(path) = worktree.which(BINARY_NAME) {
            self.cached_binary_path = Some(path.clone());
            return Ok(path);
        }

        // 2. Check ~/.cargo/bin or ~/.local/bin
        if let Ok(home) = std::env::var("HOME") {
            let cargo_bin = format!("{home}/.cargo/bin/{BINARY_NAME}");
            if fs::metadata(&cargo_bin).is_ok_and(|stat| stat.is_file()) {
                self.cached_binary_path = Some(cargo_bin.clone());
                return Ok(cargo_bin);
            }

            let local_bin = format!("{home}/.local/bin/{BINARY_NAME}");
            if fs::metadata(&local_bin).is_ok_and(|stat| stat.is_file()) {
                self.cached_binary_path = Some(local_bin.clone());
                return Ok(local_bin);
            }
        }

        // 3. Fallback error with clear actionable instructions
        Err(format!(
            "'{BINARY_NAME}' not found in PATH, ~/.cargo/bin, or ~/.local/bin. Run 'cargo install --path lsp' in the crate-helper repository or run 'make install'."
        ))
    }
}

impl zed::Extension for CrateHelperExtension {
    fn new() -> Self {
        Self {
            cached_binary_path: None,
        }
    }

    fn language_server_command(
        &mut self,
        language_server_id: &LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<zed::Command> {
        let binary_path = self.language_server_binary_path(language_server_id, worktree)?;
        Ok(zed::Command {
            command: binary_path,
            args: Vec::new(),
            env: Vec::new(),
        })
    }
}

zed::register_extension!(CrateHelperExtension);
