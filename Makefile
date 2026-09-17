.PHONY: all build-lsp install-lsp build-extension install dev-link test

all: build-lsp build-extension

# Build the native LSP server
build-lsp:
	cargo build --release --manifest-path lsp/Cargo.toml

# Install the native LSP server to ~/.cargo/bin
install-lsp:
	cargo install --path lsp

# Build the Zed extension WebAssembly binary
build-extension:
	cargo build --target wasm32-wasip1 --release

# Complete installation
install: install-lsp build-extension
	@echo ""
	@echo "============================================================"
	@echo " crate-helper installed successfully!"
	@echo " 1. crate-helper-lsp is in ~/.cargo/bin/crate-helper-lsp"
	@echo " 2. In Zed, open Command Palette (Ctrl+Shift+P)"
	@echo "    and run: 'zed: install dev extension'"
	@echo "    Select this directory: $(PWD)"
	@echo "============================================================"

# Test the LSP server directly with sample input
test:
	cargo test --manifest-path lsp/Cargo.toml
