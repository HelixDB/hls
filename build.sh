cargo build --release

cp target/release/hql extension/server/helix-query-lsp
cd extension
npm run compile
npm run package -y
npm run install-extension