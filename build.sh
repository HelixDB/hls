#!/bin/bash
set -e

echo "Building HelixQL LSP server..."
cargo build --release

echo "Creating server directory..."
mkdir -p extension/server

echo "Copying LSP binary..."
cp target/release/helixql-lsp extension/server/helixql-lsp

echo "Building VS Code extension..."
cd extension
npm install
npm run compile
npm run package
npm run install-extension

echo "Build complete!"
