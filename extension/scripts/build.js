const { execSync } = require('child_process');
const fs = require('fs');
const path = require('path');

console.log('Building HelixQL LSP server...');

// Determine target based on platform
const platform = process.platform;
const arch = process.arch;

let rustTarget;
let outputName = 'helixql-lsp';

if (platform === 'win32') {
    rustTarget = 'x86_64-pc-windows-msvc';
    outputName += '.exe';
} else if (platform === 'darwin') {
    rustTarget = arch === 'arm64' ? 'aarch64-apple-darwin' : 'x86_64-apple-darwin';
} else {
    rustTarget = 'x86_64-unknown-linux-gnu';
}

// Build the Rust server
try {
    execSync(`cargo build --release --target ${rustTarget}`, {
        cwd: path.join(__dirname, '..', 'server'),
        stdio: 'inherit'
    });
} catch (error) {
    console.error('Failed to build Rust server:', error);
    process.exit(1);
}

// Copy the binary to the extension
const sourcePath = path.join(
    __dirname,
    '..',
    'server',
    'target',
    rustTarget,
    'release',
    outputName
);

const destPath = path.join(
    __dirname,
    '..',
    'client',
    'server',
    'helixql-lsp'
);

// Create server directory if it doesn't exist
const serverDir = path.dirname(destPath);
if (!fs.existsSync(serverDir)) {
    fs.mkdirSync(serverDir, { recursive: true });
}

// Copy the binary
fs.copyFileSync(sourcePath, destPath);

// Make it executable on Unix-like systems
if (platform !== 'win32') {
    fs.chmodSync(destPath, '755');
}

console.log('HelixQL LSP server built successfully!');