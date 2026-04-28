'use strict';

const path = require('path');
const { workspace, window } = require('vscode');
const { LanguageClient, TransportKind } = require('vscode-languageclient/node');

let client;

function activate(context) {
    // Find the volta binary: prefer one on PATH, fall back to env var.
    const voltaBin = process.env.VOLTA_PATH || 'volta';

    const serverOptions = {
        command: voltaBin,
        args: ['lsp'],
        transport: TransportKind.stdio,
    };

    const clientOptions = {
        documentSelector: [{ scheme: 'file', language: 'volta' }],
        synchronize: {
            fileEvents: workspace.createFileSystemWatcher('**/*.vlt'),
        },
    };

    client = new LanguageClient(
        'volta-lsp',
        'Volta Language Server',
        serverOptions,
        clientOptions,
    );

    client.start();
}

function deactivate() {
    if (client) return client.stop();
}

module.exports = { activate, deactivate };
