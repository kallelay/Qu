// Qu Language extension — run support + inline diagnostics.
//
// Plain CommonJS, no build step: `package.json` points `main` straight at
// this file, so there's nothing to compile/bundle for either the "copy the
// folder into ~/.vscode/extensions" install path or the `vsce package` path
// this extension's README already documents.
//
// Two features live here:
//   1. "Qu: Run File" (command `qu.runFile`) — shells out to `qu run`
//      (see engine/crates/qu-cli/src/main.rs's `cmd_run`) and streams
//      stdout/stderr into an Output Channel. Also passes `--emit-vars` and,
//      once the process exits, prints that JSON dump as a clearly-labeled
//      "post-run" section — NOT a debugger (see the README's Debugging
//      section for why Qu has no stepping/breakpoint protocol today).
//   2. Live parse diagnostics — debounced on open/change/save, shells out
//      to `qu parse --json` (added alongside this extension specifically so
//      there's a machine-readable diagnostic to parse here — see that
//      command's own doc comment in qu-cli/src/main.rs) and reports the
//      result via `vscode.languages.createDiagnosticCollection`, the same
//      class of UX QuStudio's own CodeEditor.tsx already gets from its
//      in-process `check_syntax` Tauri command.
'use strict';

const vscode = require('vscode');
const cp = require('child_process');
const fs = require('fs');
const path = require('path');
const os = require('os');

const DIAGNOSTIC_DEBOUNCE_MS = 400;

/** @type {vscode.OutputChannel} */
let outputChannel;
/** @type {vscode.DiagnosticCollection} */
let diagnosticCollection;
/** @type {Map<string, NodeJS.Timeout>} */
const debounceTimers = new Map();

function activate(context) {
    outputChannel = vscode.window.createOutputChannel('Qu');
    diagnosticCollection = vscode.languages.createDiagnosticCollection('qu');
    context.subscriptions.push(outputChannel, diagnosticCollection);

    context.subscriptions.push(vscode.commands.registerCommand('qu.runFile', runFile));

    context.subscriptions.push(
        vscode.workspace.onDidOpenTextDocument(scheduleCheck),
        vscode.workspace.onDidChangeTextDocument((e) => scheduleCheck(e.document)),
        vscode.workspace.onDidSaveTextDocument(scheduleCheck),
        vscode.workspace.onDidCloseTextDocument((doc) => {
            diagnosticCollection.delete(doc.uri);
            const key = doc.uri.toString();
            clearTimeout(debounceTimers.get(key));
            debounceTimers.delete(key);
        })
    );

    // Any .qu files already open when the extension activates should get
    // diagnostics immediately, not just on the next edit.
    vscode.workspace.textDocuments.forEach(scheduleCheck);
}

function deactivate() {
    debounceTimers.forEach((t) => clearTimeout(t));
    debounceTimers.clear();
}

// ------------------------------------------------------------------ run

/**
 * `qu.runFile`. Saves the active .qu document if dirty (so what runs
 * matches what's on disk — `qu run` only ever reads from a real path),
 * then streams `qu run --emit-vars <tmp> <file>`'s output into the "Qu"
 * Output Channel. On exit, if the temp vars file was written, appends its
 * contents as a labeled post-run variable dump (see module doc comment).
 */
async function runFile() {
    const editor = vscode.window.activeTextEditor;
    if (!editor || editor.document.languageId !== 'qu') {
        vscode.window.showWarningMessage('Qu: Run File only works on an open .qu file.');
        return;
    }

    const quPath = resolveQuExecutable();
    if (!quPath) {
        reportMissingExecutable();
        return;
    }

    const document = editor.document;
    if (document.isDirty) {
        await document.save();
    }
    const filePath = document.fileName;
    const varsPath = path.join(
        os.tmpdir(),
        `qu-vscode-vars-${process.pid}-${Date.now()}-${Math.random().toString(36).slice(2)}.json`
    );

    outputChannel.show(true);
    outputChannel.appendLine(`--- qu run "${filePath}" ---`);

    let child;
    try {
        child = cp.spawn(quPath, ['run', '--emit-vars', varsPath, filePath], {
            cwd: path.dirname(filePath),
        });
    } catch (err) {
        outputChannel.appendLine(`[failed to start qu: ${err.message}]`);
        reportMissingExecutable();
        return;
    }

    child.stdout.on('data', (d) => outputChannel.append(d.toString()));
    child.stderr.on('data', (d) => outputChannel.append(d.toString()));
    child.on('error', (err) => {
        outputChannel.appendLine(`[qu run failed to start: ${err.message}]`);
        reportMissingExecutable();
    });
    child.on('close', (code) => {
        outputChannel.appendLine(`--- qu exited with code ${code} ---`);
        appendVarsDumpIfPresent(varsPath);
    });
}

/** Best-effort: read+delete the `--emit-vars` JSON temp file and print it. */
function appendVarsDumpIfPresent(varsPath) {
    fs.readFile(varsPath, 'utf8', (err, raw) => {
        if (err) return; // e.g. the run errored before producing any vars — not a problem
        fs.unlink(varsPath, () => {});
        let vars;
        try {
            vars = JSON.parse(raw);
        } catch (_) {
            return;
        }
        if (!Array.isArray(vars) || vars.length === 0) return;
        outputChannel.appendLine('');
        outputChannel.appendLine(
            '-- Variables after the script finished (post-run snapshot, not a live debugger) --'
        );
        for (const v of vars) {
            outputChannel.appendLine(`  ${v.name} : ${v.type} = ${v.preview}`);
        }
    });
}

// ------------------------------------------------------------------ diagnostics

function scheduleCheck(document) {
    if (!document || document.languageId !== 'qu') return;
    if (!vscode.workspace.getConfiguration('qu').get('diagnostics.enable', true)) {
        diagnosticCollection.delete(document.uri);
        return;
    }
    const key = document.uri.toString();
    clearTimeout(debounceTimers.get(key));
    debounceTimers.set(
        key,
        setTimeout(() => checkDocument(document), DIAGNOSTIC_DEBOUNCE_MS)
    );
}

/**
 * Runs `qu parse --json` against the document's CURRENT in-editor text
 * (not necessarily what's saved on disk), so diagnostics update as the user
 * types rather than only on save. Since `qu parse` only reads real files,
 * the text is written to a per-document scratch file in the OS temp dir
 * first; that file is deleted again once the check completes.
 */
function checkDocument(document) {
    const quPath = resolveQuExecutable();
    if (!quPath) return; // stay quiet here; `qu.runFile` is what surfaces the missing-executable error

    const tmpFile = path.join(os.tmpdir(), `qu-vscode-check-${hashUri(document.uri)}.qu`);
    fs.writeFile(tmpFile, document.getText(), 'utf8', (writeErr) => {
        if (writeErr) return;
        cp.execFile(quPath, ['parse', '--json', tmpFile], { timeout: 5000 }, (_error, stdout) => {
            fs.unlink(tmpFile, () => {});
            if (!stdout) return;
            let payload;
            try {
                payload = JSON.parse(stdout);
            } catch (_) {
                return; // unparseable output shouldn't crash the extension; just skip this round
            }
            const diagnostics = (payload.diagnostics || []).map((d) => {
                const line = Math.max(0, (d.line || 1) - 1);
                const col = Math.max(0, (d.column || 1) - 1);
                const range = new vscode.Range(line, col, line, col + 1);
                return new vscode.Diagnostic(range, d.message, vscode.DiagnosticSeverity.Error);
            });
            diagnosticCollection.set(document.uri, diagnostics);
        });
    });
}

/** Small stable hash (no crypto dependency) used only to name temp files. */
function hashUri(uri) {
    let h = 0;
    const s = uri.toString();
    for (let i = 0; i < s.length; i++) {
        h = (h * 31 + s.charCodeAt(i)) | 0;
    }
    return (h >>> 0).toString(16);
}

// ------------------------------------------------------------------ executable resolution

/**
 * Resolves the `qu` executable: an explicit `qu.executablePath` setting
 * always wins (returns null if that path doesn't exist, rather than
 * silently falling back — a configured-but-wrong path should be a clear
 * error, not a mysterious PATH lookup succeeding or failing instead).
 * Otherwise searches `PATH` (with `PATHEXT` on Windows) for `qu`, mirroring
 * the layered lookup `qu-studio-tauri`'s `find_qu_executable` already does
 * for the same problem.
 */
function resolveQuExecutable() {
    const configured = (vscode.workspace.getConfiguration('qu').get('executablePath') || '').trim();
    if (configured) {
        return fs.existsSync(configured) ? configured : null;
    }
    return findOnPath('qu');
}

function findOnPath(baseName) {
    const pathEnv = process.env.PATH || process.env.Path || '';
    const dirs = pathEnv.split(path.delimiter).filter(Boolean);
    const exts =
        process.platform === 'win32' ? (process.env.PATHEXT || '.EXE;.CMD;.BAT').split(';') : [''];
    for (const dir of dirs) {
        for (const ext of exts) {
            const candidate = path.join(dir, baseName + ext);
            try {
                if (fs.statSync(candidate).isFile()) {
                    return candidate;
                }
            } catch (_) {
                // not found here — keep looking
            }
        }
    }
    return null;
}

function reportMissingExecutable() {
    const configured = (vscode.workspace.getConfiguration('qu').get('executablePath') || '').trim();
    const message = configured
        ? `Qu: the configured qu.executablePath ("${configured}") does not exist.`
        : 'Qu: could not find a "qu"/"qu.exe" executable on PATH.';
    vscode.window.showErrorMessage(message, 'Open Settings').then((choice) => {
        if (choice === 'Open Settings') {
            vscode.commands.executeCommand('workbench.action.openSettings', 'qu.executablePath');
        }
    });
}

module.exports = { activate, deactivate };
