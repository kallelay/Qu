// Self-test for lspClient.js against a real `qu` binary. Plain node, no
// dependencies, no VS Code:
//
//     node editors/vscode-qu/test/lsp_client_selftest.js <path-to-qu>
//
// Exits 0 only if every check passed. Not shipped with the extension.
'use strict';

const assert = require('assert');
const path = require('path');
const { QuLspClient } = require('../lspClient');

const qu = process.argv[2];
if (!qu) {
    console.error('usage: node lsp_client_selftest.js <path-to-qu>');
    process.exit(2);
}

const URI = 'file:///selftest.qu';
// Non-ASCII on purpose: Content-Length must count bytes, in both directions.
const TEXT = ['# µ-volt scaling — "é"', 'function scale(x)', '    return x * 1e-6', 'end function', 'y = scale(butter(2, "low", 5, 100)', ''].join('\n');

function nextDiagnostics(client) {
    return new Promise((resolve, reject) => {
        const timer = setTimeout(() => reject(new Error('no publishDiagnostics within 10 s')), 10000);
        const on = (method, params) => {
            if (method !== 'textDocument/publishDiagnostics') return;
            clearTimeout(timer);
            client.removeListener('notification', on);
            resolve(params);
        };
        client.on('notification', on);
    });
}

async function main() {
    const client = new QuLspClient(qu);
    const stderr = [];
    client.on('stderr', (t) => stderr.push(t));
    const init = await client.start({
        textDocument: { documentSymbol: { hierarchicalDocumentSymbolSupport: true } },
    });
    assert.strictEqual(init.capabilities.hoverProvider, true);
    assert.strictEqual(client.capabilities.textDocumentSync.change, 1);
    console.log('initialize ok:', JSON.stringify(init.serverInfo));

    const pending = nextDiagnostics(client);
    client.notify('textDocument/didOpen', { textDocument: { uri: URI, languageId: 'qu', version: 1, text: TEXT } });
    const diags = await pending;
    assert.strictEqual(diags.uri, URI);
    assert.strictEqual(diags.diagnostics.length, 1, JSON.stringify(diags));
    console.log('diagnostic:', JSON.stringify(diags.diagnostics[0]));

    const hover = await client.request('textDocument/hover', { textDocument: { uri: URI }, position: { line: 4, character: 12 } });
    assert.ok(hover.contents.value.includes('butter(order, kind, cutoff, fs)'), JSON.stringify(hover));
    console.log('hover ok');

    const completion = await client.request('textDocument/completion', { textDocument: { uri: URI }, position: { line: 4, character: 6 } });
    assert.ok(completion.items.some((i) => i.label === 'scale'), 'user function offered');
    console.log('completion ok:', completion.items.length, 'items for prefix "sc"');

    const symbols = await client.request('textDocument/documentSymbol', { textDocument: { uri: URI } });
    assert.strictEqual(symbols[0].name, 'scale');
    console.log('documentSymbol ok');

    // An error reply rejects, with the code attached.
    await assert.rejects(client.request('textDocument/rename', {}), (e) => e.code === -32601);

    // Two requests in flight resolve to their own answers.
    const [a, b] = await Promise.all([
        client.request('textDocument/hover', { textDocument: { uri: URI }, position: { line: 4, character: 12 } }),
        client.request('textDocument/definition', { textDocument: { uri: URI }, position: { line: 4, character: 6 } }),
    ]);
    assert.ok(a.contents && b.range && b.range.start.line === 1, JSON.stringify([a, b]));

    const exited = new Promise((resolve) => client.once('exit', resolve));
    await client.stop();
    assert.strictEqual(await exited, 0, 'shutdown + exit is exit code 0');
    await assert.rejects(client.request('textDocument/hover', {}), /not running/);
    console.log('shutdown/exit ok (exit code 0)');

    // A binary that is not an LSP server must reject start(), not hang.
    const notAServer = new QuLspClient(qu, ['definitely-not-a-command']);
    await assert.rejects(notAServer.start({}), /exited before initializing/);
    const missing = new QuLspClient(path.join(__dirname, 'no-such-qu-binary'));
    missing.on('error', () => {});
    await assert.rejects(missing.start({}));
    console.log('start() rejects for a non-server and for a missing executable');

    console.log('ALL OK');
}

main().catch((err) => {
    console.error('FAILED:', err && err.stack ? err.stack : err);
    process.exit(1);
});
