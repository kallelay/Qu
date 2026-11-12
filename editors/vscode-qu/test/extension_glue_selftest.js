// Self-test for extension.js's language-server glue, with a STUB of the
// `vscode` module and a REAL `qu lsp`:
//
//     node editors/vscode-qu/test/extension_glue_selftest.js <path-to-qu>
//
// What this does and does not show: it runs the extension's own activate(),
// document-sync handlers and provider functions and checks the objects they
// hand back to "VS Code". It cannot show that a real VS Code accepts those
// objects or renders them — the stub's classes only record their arguments.
// Not shipped with the extension.
'use strict';

const assert = require('assert');
const Module = require('module');
const path = require('path');

const qu = process.argv[2];
if (!qu) {
    console.error('usage: node extension_glue_selftest.js <path-to-qu>');
    process.exit(2);
}

// ---------------------------------------------------------------- vscode stub

class Range {
    constructor(sl, sc, el, ec) {
        this.start = { line: sl, character: sc };
        this.end = { line: el, character: ec };
    }
}
class Uri {
    constructor(s) {
        this.s = s;
        this.fsPath = s;
    }
    toString() {
        return this.s;
    }
    static parse(s) {
        return new Uri(s);
    }
    static file(p) {
        return new Uri('file:///' + p.replace(/\\/g, '/'));
    }
}
const record = (name, fields) =>
    class {
        constructor(...args) {
            this._class = name;
            fields.forEach((f, i) => (this[f] = args[i]));
        }
    };

const config = { executablePath: qu, 'diagnostics.enable': true, 'languageServer.enable': true };
const handlers = {};
const providers = {};
const output = [];
const published = new Map();
const on = (name) => (fn) => {
    handlers[name] = fn;
    return { dispose() {} };
};
const provider =
    (name) =>
    (selector, impl, ...rest) => {
        assert.deepStrictEqual(selector, { language: 'qu' });
        providers[name] = { impl, rest };
        return {
            dispose() {
                delete providers[name];
            },
        };
    };

const vscode = {
    Range,
    Uri,
    Hover: record('Hover', ['contents', 'range']),
    MarkdownString: record('MarkdownString', ['value']),
    CompletionItem: record('CompletionItem', ['label', 'kind']),
    CompletionList: record('CompletionList', ['items', 'isIncomplete']),
    SignatureHelp: record('SignatureHelp', []),
    SignatureInformation: record('SignatureInformation', ['label', 'documentation']),
    ParameterInformation: record('ParameterInformation', ['label', 'documentation']),
    DocumentSymbol: record('DocumentSymbol', ['name', 'detail', 'kind', 'range', 'selectionRange']),
    Location: record('Location', ['uri', 'range']),
    Diagnostic: record('Diagnostic', ['range', 'message', 'severity']),
    DiagnosticSeverity: { Error: 0, Warning: 1, Information: 2, Hint: 3 },
    ConfigurationTarget: { Global: 1 },
    ViewColumn: { Beside: -2 },
    workspace: {
        textDocuments: [],
        getConfiguration: () => ({ get: (key, dflt) => (key in config ? config[key] : dflt) }),
        onDidOpenTextDocument: on('open'),
        onDidChangeTextDocument: on('change'),
        onDidSaveTextDocument: on('save'),
        onDidCloseTextDocument: on('close'),
        onDidChangeConfiguration: on('config'),
    },
    languages: {
        createDiagnosticCollection: () => ({
            set: (uri, d) => published.set(uri.toString(), d),
            delete: (uri) => published.delete(uri.toString()),
            clear: () => published.clear(),
            dispose() {},
        }),
        registerHoverProvider: provider('hover'),
        registerCompletionItemProvider: provider('completion'),
        registerSignatureHelpProvider: provider('signatureHelp'),
        registerDocumentSymbolProvider: provider('documentSymbol'),
        registerDefinitionProvider: provider('definition'),
    },
    window: {
        createOutputChannel: () => ({
            append: (t) => output.push(t),
            appendLine: (t) => output.push(t + '\n'),
            show() {},
            dispose() {},
        }),
        showErrorMessage: () => Promise.resolve(undefined),
        showWarningMessage: () => Promise.resolve(undefined),
        activeTextEditor: undefined,
    },
    commands: { registerCommand: () => ({ dispose() {} }), executeCommand: () => Promise.resolve() },
};

const realLoad = Module._load;
Module._load = function (request, ...rest) {
    return request === 'vscode' ? vscode : realLoad.call(this, request, ...rest);
};

function fakeDocument(uri, text) {
    return {
        uri: Uri.parse(uri),
        languageId: 'qu',
        version: 1,
        fileName: uri,
        isDirty: false,
        getText() {
            return this.text;
        },
        text,
    };
}

async function until(what, predicate) {
    for (let i = 0; i < 200; i++) {
        if (predicate()) return;
        await new Promise((r) => setTimeout(r, 25));
    }
    throw new Error(`timed out waiting for ${what}; output channel: ${output.join('')}`);
}

async function main() {
    const ext = require(path.join(__dirname, '..', 'extension.js'));
    const URI = 'file:///glue.qu';
    const doc = fakeDocument(URI, ['#%% Cell one', 'function scale(x)', '    return x * 2', 'end function', 'y = scale(butter(2, "low", 5, 100)', ''].join('\n'));
    vscode.workspace.textDocuments.push(doc);

    await ext.activate({ extensionPath: path.join(__dirname, '..'), subscriptions: [] });
    assert.deepStrictEqual(Object.keys(providers).sort(), ['completion', 'definition', 'documentSymbol', 'hover', 'signatureHelp'], output.join(''));
    assert.deepStrictEqual(providers.completion.rest, ['.']);
    assert.deepStrictEqual(providers.signatureHelp.rest, ['(', ',']);

    // The document that was already open got didOpen, and its diagnostics arrive.
    await until('diagnostics for the open document', () => (published.get(URI) || []).length === 1);
    const d = published.get(URI)[0];
    assert.strictEqual(d.severity, 0, 'LSP severity 1 -> vscode.DiagnosticSeverity.Error');
    assert.strictEqual(d.source, 'qu');
    assert.ok(d.range instanceof Range);
    console.log('diagnostic via server:', JSON.stringify(d));

    const pos = (line, character) => ({ line, character });
    const hover = await providers.hover.impl.provideHover(doc, pos(4, 12));
    assert.strictEqual(hover._class, 'Hover');
    assert.strictEqual(hover.contents._class, 'MarkdownString');
    assert.ok(hover.contents.value.includes('butter(order, kind, cutoff, fs)'));
    assert.deepStrictEqual(hover.range.start, { line: 4, character: 10 });
    assert.strictEqual(await providers.hover.impl.provideHover(doc, pos(4, 1)), undefined, 'no hover on whitespace');

    const list = await providers.completion.impl.provideCompletionItems(doc, pos(4, 6));
    assert.strictEqual(list._class, 'CompletionList');
    const scale = list.items.find((i) => i.label === 'scale');
    assert.ok(scale, 'user function offered');
    assert.strictEqual(scale.kind, 2, 'LSP Function (3) -> vscode.CompletionItemKind.Function (2)');
    assert.strictEqual(scale.detail, 'scale(x)');

    const help = await providers.signatureHelp.impl.provideSignatureHelp(doc, pos(4, 27));
    assert.strictEqual(help.signatures[0].label, 'butter(order, kind, cutoff, fs)');
    assert.strictEqual(help.activeParameter, 2);
    assert.deepStrictEqual(help.signatures[0].parameters[2].label, [20, 26]);

    const symbols = await providers.documentSymbol.impl.provideDocumentSymbols(doc);
    assert.strictEqual(symbols[0].name, 'Cell one');
    assert.strictEqual(symbols[0].kind, 1, 'LSP Module (2) -> vscode.SymbolKind.Module (1)');
    assert.strictEqual(symbols[0].children[0].name, 'scale');
    assert.strictEqual(symbols[0].children[0].kind, 11, 'LSP Function (12) -> vscode.SymbolKind.Function (11)');

    const def = await providers.definition.impl.provideDefinition(doc, pos(4, 6));
    assert.strictEqual(def._class, 'Location');
    assert.strictEqual(def.uri.toString(), URI);
    assert.deepStrictEqual(def.range.start, { line: 1, character: 9 });
    console.log('hover / completion / signatureHelp / documentSymbol / definition ok');

    // An edit that fixes the file clears the diagnostic (full-text didChange).
    doc.text = doc.text.replace('100)', '100))');
    doc.version = 2;
    handlers.change({ document: doc });
    await until('the diagnostic to clear after the fix', () => (published.get(URI) || []).length === 0);
    console.log('didChange ok: diagnostics cleared');

    // A document opened later is synced too; closing clears it.
    const other = fakeDocument('file:///other.qu', 'x = (\n');
    handlers.open(other);
    await until('diagnostics for a newly opened document', () => (published.get('file:///other.qu') || []).length === 1);
    handlers.close(other);
    assert.strictEqual(published.has('file:///other.qu'), false);
    console.log('didOpen / didClose ok');

    // A `qu` that has no `lsp` command: no providers, one explanatory line, no throw.
    config.executablePath = process.execPath; // node: exists, is not a language server
    handlers.config({ affectsConfiguration: (k) => k === 'qu.executablePath' });
    await until('the did-not-start message', () => output.join('').includes('qu lsp did not start'));
    assert.deepStrictEqual(Object.keys(providers), [], 'providers are disposed when the server cannot start');
    console.log('fallback ok: providers removed, message logged');

    // Turning it back on restores everything.
    config.executablePath = qu;
    handlers.config({ affectsConfiguration: (k) => k === 'qu.executablePath' });
    await until('the providers to come back', () => Object.keys(providers).length === 5);
    await until('diagnostics after restart', () => published.has(URI));

    await ext.deactivate();
    assert.deepStrictEqual(Object.keys(providers), []);
    console.log('ALL OK');
    process.exit(0);
}

main().catch((err) => {
    console.error('FAILED:', err && err.stack ? err.stack : err);
    process.exit(1);
});
