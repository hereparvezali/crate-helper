# 📦 Crate Helper — Zed Editor Extension

A modern, high-performance Zed extension that supercharges your `Cargo.toml` workflow with:
- ⚡ **Intelligent Autocomplete**: Instant crate name suggestions and version completion sorted latest-first.
- 🔍 **Hover Version Suggestions**: Rich markdown tooltip displaying crate description, links, and a table of all the latest releases.
- 🔄 **Updated & Backdated Detection**: In-editor inlay hints (`✓` or `⭡ 1.0.229`) and diagnostic warnings for outdated dependencies.
- 💡 **Quick Fix Code Actions**: One-click updates to bump outdated crates directly to their latest version.

---

## ✨ Features

### 1. Autocomplete for `Cargo.toml`
* **Crate Names**: Type under `[dependencies]`, `[dev-dependencies]`, `[build-dependencies]`, or `[workspace.dependencies]` to get instant autocompletion of popular Rust crates (plus dynamic search from crates.io).
* **Crate Versions**: Typing inside a version string (e.g. `serde = "1."` or `{ version = "" }`) displays all available versions fetched directly from the crates.io sparse index, sorted with the latest stable version first.

### 2. Version Information on Hover (Click-to-Replace)
Hover over any crate name or version in your `Cargo.toml` to view:
* **Minimalist Design**: Only the crate name is displayed at the top.
* **Clickable Versions**: Shows the last 20 releases.
* **Instant Replacement**: Clicking any version link immediately replaces the existing version in your `Cargo.toml`!
* **Quick Fix Integration**: You can also use Zed's Code Action / Quick Fix menu (<kbd>Alt</kbd>+<kbd>Enter</kbd> / <kbd>Ctrl</kbd>+<kbd>.</kbd>) to pick from the last 20 versions.

### 3. Clean Inline Version Hints (No Warning Squiggles)
* **Non-intrusive Inlay Hints**: Shows clean inline text next to each dependency line:
  * `✓` if the crate is up to date.
  * `⭡ 1.0.229` if a newer version is available.
* **No Annoying Warnings**: No red/yellow error squiggles or warning popups cluttering your code.
* **Quick Fixes**: Trigger code actions (<kbd>Alt</kbd>+<kbd>Enter</kbd> / <kbd>Ctrl</kbd>+<kbd>.</kbd>) on any dependency to pick and replace any of the 20 versions.

---

## 🚀 Quick Start

### 1. Build and Install the LSP Server
The extension communicates with `crate-helper-lsp`, a native language server. Install it to your Cargo bin directory:

```bash
cd /home/parvez/reps/crate-helper
make install
```

Or manually:
```bash
cargo install --path lsp
cargo build --target wasm32-wasip1 --release
```

This places `crate-helper-lsp` into `~/.cargo/bin/crate-helper-lsp` (which is in your `PATH`).

### 2. Install Dev Extension in Zed

1. Open Zed (`zeditor`).
2. Press <kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>P</kbd> (or <kbd>Cmd</kbd> + <kbd>Shift</kbd> + <kbd>P</kbd> on macOS) to open the Command Palette.
3. Type and select:
   ```
   zed: install dev extension
   ```
4. Select the `/home/parvez/reps/crate-helper` directory.

Zed will compile the WebAssembly extension and activate `Crate Helper` for all TOML files!

---

## 🧪 Testing

You can test the extension with the included fixture:

1. Open Zed:
   ```bash
   zeditor /home/parvez/reps/crate-helper/test-fixtures/Cargo.toml
   ```
2. Observe:
   - Inlay hints showing `⭡ <latest>` for outdated crates like `serde = "1.0.100"` and `tokio = { version = "1.0.0" }`.
   - Hover over `serde` or `1.0.100` to see the rich version list.
   - Type a new dependency like `cla` to see `clap` autocompletion.
   - Inside `serde = "1."`, trigger completions to view all available `1.x` versions.

---

## 📂 Project Architecture

```
crate-helper/
├── extension.toml            # Zed extension manifest registered for TOML
├── Cargo.toml                # Root package: WebAssembly extension (wasm32-wasip1 / wasip2)
├── src/
│   └── lib.rs                # Zed extension host bindings (spawns crate-helper-lsp)
├── lsp/                      # Language Server Protocol (LSP) implementation
│   ├── Cargo.toml            # Dependencies: tower-lsp, tokio, reqwest, toml_edit, semver
│   └── src/
│       ├── main.rs           # LSP entry point and stdin/stdout server runner
│       ├── server.rs         # Hover, completions, inlay hints, diagnostics, code actions
│       ├── crates_client.rs  # Sparse index client (fast CDN) and crates.io metadata
│       ├── toml_parser.rs    # Resilient line & AST parser for Cargo.toml
│       └── popular_crates.rs # Instant 0ms autocomplete for top Rust crates
├── test-fixtures/
│   └── Cargo.toml            # Sample file for testing all features
└── Makefile                  # Build and installation targets
```

---

## ⚙️ Configuration (Optional)

Ensure inlay hints and inline diagnostics are enabled in your Zed `settings.json`:

```json
{
  "inlay_hints": {
    "enabled": true
  },
  "diagnostics": {
    "inline": {
      "enabled": true
    }
  }
}
```
