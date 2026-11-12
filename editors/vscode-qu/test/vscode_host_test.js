// Runs INSIDE a real VS Code extension host and exercises the language
// features through VS Code's own provider commands. Not shipped.
//
// Launch (a throw-away profile, so the user's own VS Code is untouched):
//
//   Code.exe --extensionDevelopmentPath=<repo>/editors/vscode-qu
//            --extensionTestsPath=<repo>/editors/vscode-qu/test/vscode_host_test.js
//            --user-data-dir=<tmp>/ud --extensions-dir=<tmp>/ext
//            --disable-workspace-trust --skip-welcome --disable-updates <file.qu>
//
// with <tmp>/ud/User/settings.json holding {"qu.executablePath": "<path-to-qu>"}
// and these environment variables:
//
//   QU_LSP_TEST_FILE   the .qu file to open (contents: FIXTURE below)
//   QU_LSP_TEST_OUT    where to write the JSON report
//
// VS Code exits 0 if run() resolves and 1 if it rejects.
'use strict';

const assert = require('assert');
const fs = require('fs');
const vscode = require('vscode');

const FIXTURE = ['#%% Cell one', 'function scale(x)', '    return x * 2', 'end function', 'y = scale(butter(2, "low", 5, 100)', ''].join('\n');

async function until(what, predicate) {
    for (let i = 0; i < 400; i++) {
        if (await predicate()) return;
        await new Promise((r) => setTimeout(r, 50));
    }
    throw new Error(`timed out waiting for ${what}`);
}

function hoverText(hovers) {
    return (hovers || [])
        .map((h) => h.contents.map((c) => (typeof c === 'string' ? c : c.value)).join('\n'))
        .join('\n');
}

exports.run = async function run() {
    const report = { vscode: vscode.version, steps: [] };
    const step = (name, detail) => report.steps.push({ name, detail });
    try {
        fs.writeFileSync(process.env.QU_LSP_TEST_FILE, FIXTURE);
        const doc = await vscode.workspace.openTextDocument(vscode.Uri.file(process.env.QU_LSP_TEST_FILE));
        assert.strictEqual(doc.languageId, 'qu');
        const editor = await vscode.window.showTextDocument(doc);
        const ext = vscode.extensions.getExtension('qu-project.qu-language');
        assert.ok(ext, 'the extension under development is loaded');
        await ext.activate();
        step('activate', { quExecutablePath: vscode.workspace.getConfiguration('qu').get('executablePath') });

        await until('a diagnostic from the server', () => vscode.languages.getDiagnostics(doc.uri).length > 0);
        const diags = vscode.languages.getDiagnostics(doc.uri);
        assert.strictEqual(diags.length, 1);
        assert.strictEqual(diags[0].source, 'qu', 'source "qu" marks the language-server path (the fallback sets none)');
        assert.strictEqual(diags[0].severity, vscode.DiagnosticSeverity.Error);
        step('diagnostics', { message: diags[0].message, line: diags[0].range.start.line, character: diags[0].range.start.character });

        const hovers = await vscode.commands.executeCommand('vscode.executeHoverProvider', doc.uri, new vscode.Position(4, 12));
        const text = hoverText(hovers);
        assert.ok(text.includes('butter(order, kind, cutoff, fs)'), `hover: ${text}`);
        step('hover builtin', text);
        const userHover = hoverText(await vscode.commands.executeCommand('vscode.executeHoverProvider', doc.uri, new vscode.Position(4, 6)));
        assert.ok(userHover.includes('function scale(x)'), `hover: ${userHover}`);
        step('hover user function', userHover);

        const list = await vscode.commands.executeCommand('vscode.executeCompletionItemProvider', doc.uri, new vscode.Position(4, 6));
        const label = (i) => (typeof i.label === 'string' ? i.label : i.label.label);
        const scale = list.items.find((i) => label(i) === 'scale' && i.detail === 'scale(x)');
        assert.ok(scale, 'the user function is offered with its header as detail');
        assert.strictEqual(scale.kind, vscode.CompletionItemKind.Function);
        // Type `but` on a new line and ask again: the builtin, with its signature.
        await editor.edit((b) => b.insert(new vscode.Position(5, 0), 'but'));
        const list2 = await vscode.commands.executeCommand('vscode.executeCompletionItemProvider', doc.uri, new vscode.Position(5, 3));
        const butter = list2.items.find((i) => label(i) === 'butter' && i.detail === 'butter(order, kind, cutoff, fs)');
        assert.ok(butter, `butter offered for "but": ${list2.items.slice(0, 8).map(label)}`);
        assert.strictEqual(butter.kind, vscode.CompletionItemKind.Function);
        step('completion', { forSc: list.items.filter((i) => i.detail).length, butterDetail: butter.detail, butterDoc: butter.documentation && butter.documentation.value });

        const help = await vscode.commands.executeCommand('vscode.executeSignatureHelpProvider', doc.uri, new vscode.Position(4, 27), ',');
        assert.strictEqual(help.signatures[0].label, 'butter(order, kind, cutoff, fs)');
        assert.strictEqual(help.activeParameter, 2);
        step('signatureHelp', { label: help.signatures[0].label, activeParameter: help.activeParameter, parameter: help.signatures[0].parameters[2].label });

        const symbols = await vscode.commands.executeCommand('vscode.executeDocumentSymbolProvider', doc.uri);
        assert.strictEqual(symbols[0].name, 'Cell one');
        assert.strictEqual(symbols[0].kind, vscode.SymbolKind.Module);
        assert.strictEqual(symbols[0].children[0].name, 'scale');
        assert.strictEqual(symbols[0].children[0].kind, vscode.SymbolKind.Function);
        step('documentSymbol', symbols.map((s) => ({ name: s.name, children: s.children.map((c) => c.name) })));

        const defs = await vscode.commands.executeCommand('vscode.executeDefinitionProvider', doc.uri, new vscode.Position(4, 6));
        const target = defs[0].targetRange || defs[0].range;
        assert.strictEqual(target.start.line, 1);
        assert.strictEqual(target.start.character, 9);
        step('definition', { line: target.start.line, character: target.start.character });

        // Fix the file: the diagnostic must go away (full-text didChange).
        await editor.edit((b) => {
            b.delete(new vscode.Range(5, 0, 5, 3));
            b.insert(new vscode.Position(4, 34), ')');
        });
        await until('the diagnostic to clear', () => vscode.languages.getDiagnostics(doc.uri).length === 0);
        step('didChange clears diagnostics', true);
        report.ok = true;
    } catch (err) {
        report.ok = false;
        report.error = String(err && err.stack ? err.stack : err);
        throw err;
    } finally {
        fs.writeFileSync(process.env.QU_LSP_TEST_OUT, JSON.stringify(report, null, 2));
        await vscode.commands.executeCommand('workbench.action.files.revert').then(
            () => {},
            () => {}
        );
    }
};
