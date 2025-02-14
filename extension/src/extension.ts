import * as path from 'path';
import { workspace, ExtensionContext } from 'vscode';
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
} from 'vscode-languageclient/node';

let client: LanguageClient;

export function activate(context: ExtensionContext) {
  const serverPath = context.asAbsolutePath(
    path.join('server', 'helix-query-lsp')
  );

  const serverOptions: ServerOptions = {
    run: { command: serverPath },
    debug: { command: serverPath, args: ['--debug'] }
  };

  const clientOptions: LanguageClientOptions = {
    documentSelector: [{ scheme: 'file', language: 'helixquery' }],
    synchronize: {
      fileEvents: workspace.createFileSystemWatcher('**/*.hx')
    }
  };

  client = new LanguageClient(
    'helixQueryLanguageServer',
    'Helix Query Language Server',
    serverOptions,
    clientOptions
  );

  client.start();
}

export function deactivate(): Thenable<void> | undefined {
  if (!client) {
    return undefined;
  }
  return client.stop();
}