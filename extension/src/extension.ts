import * as path from 'path';
import { workspace, ExtensionContext } from 'vscode';
import {
    LanguageClient,
    LanguageClientOptions,
    ServerOptions,
    TransportKind
} from 'vscode-languageclient/node';

let client: LanguageClient;

export function activate(context: ExtensionContext) {
    console.log('HelixQL extension activating...');

    // Path to the bundled server binary
    const serverModule = context.asAbsolutePath(
        path.join('server', 'helixql-lsp')
    );

    // Server options
    const serverOptions: ServerOptions = {
        run: {
            command: serverModule,
            transport: TransportKind.stdio
        },
        debug: {
            command: serverModule,
            transport: TransportKind.stdio,
            options: {
                env: {
                    ...process.env,
                    RUST_LOG: 'debug'
                }
            }
        }
    };

    // Client options
    const clientOptions: LanguageClientOptions = {
        documentSelector: [
            { scheme: 'file', language: 'helixquery' },
            { scheme: 'file', pattern: '**/*.hx' },
            { scheme: 'file', pattern: '**/*.hql' }
        ],
        synchronize: {
            fileEvents: workspace.createFileSystemWatcher('**/*.{hx,hql}')
        }
    };

    // Create the language client and start it
    client = new LanguageClient(
        'helixql',
        'HelixQL Language Server',
        serverOptions,
        clientOptions
    );

    // Start the client
    client.start();
    console.log('HelixQL Language Server started');
}

export function deactivate(): Thenable<void> | undefined {
    if (!client) {
        return undefined;
    }
    return client.stop();
}
