// Qu Language extension — run support + inline diagnostics.
// Qu editor plugin 0.2.2
//
// Plain CommonJS, no build step: `package.json` points `main` straight at
// this file, so there's nothing to compile/bundle for either the "copy the
// folder into ~/.vscode/extensions" install path or the `vsce package` path
// this extension's README already documents.
//
// Features that live here:
//   0. Language intelligence — hover, completion, signature help, outline
//      (Ctrl+Shift+O, breadcrumbs), go-to-definition and diagnostics, all
//      served by `qu lsp` (engine/crates/qu-cli/src/lsp.rs) through the
//      small client in lspClient.js. If the `qu` found is too old to have
//      `lsp`, or the server stops, feature 2 below takes over diagnostics
//      and the rest is simply absent.
//   1. "Qu: Run File" (command `qu.runFile`) — shells out to `qu run`
//      (see engine/crates/qu-cli/src/main.rs's `cmd_run`) and streams
//      stdout/stderr into an Output Channel. Also passes `--emit-vars` and,
//      once the process exits, prints that JSON dump as a clearly-labeled
//      "post-run" section — NOT a debugger (see the README's Debugging
//      section for why Qu has no stepping/breakpoint protocol today).
//   2. Live parse diagnostics (the fallback when `qu lsp` is not running) — debounced on open/change/save, shells out
//      to `qu parse --json` (added alongside this extension specifically so
//      there's a machine-readable diagnostic to parse here — see that
//      command's own doc comment in qu-cli/src/main.rs) and reports the
//      result via `vscode.languages.createDiagnosticCollection`, the same
//      class of UX QuStudio's own CodeEditor.tsx already gets from its
//      in-process `check_syntax` Tauri command.
//   3. "Qu: HTML Diff With File..." (command `qu.htmlDiff`) — shells out to
//      the bundled `scripts/qu_htmldiff.qu` (a Qu port of the project's
//      `htmldiff.py`, per the "Qu tooling must be Qu" standing practice) to
//      build a side-by-side word-level HTML diff of the active file against
//      a second file the user picks, then shows it in a webview panel. Not
//      a source-control diff — a general two-file comparison tool, so it
//      works on any file type, not just .qu.
'use strict';

const vscode = require('vscode');
const cp = require('child_process');
const fs = require('fs');
const path = require('path');
const os = require('os');
const { QuLspClient } = require('./lspClient');

const DIAGNOSTIC_DEBOUNCE_MS = 400;

/** @type {vscode.OutputChannel} */
let outputChannel;
/** @type {vscode.DiagnosticCollection} */
let diagnosticCollection;
/** @type {vscode.DiagnosticCollection} runtime-error frames from the last `qu run` */
let runDiagnosticCollection;
/** @type {Map<string, NodeJS.Timeout>} */
const debounceTimers = new Map();
/** @type {string} set once in activate(); needed to locate the bundled scripts/ dir */
let extensionPath;
/** @type {QuLspClient | null} the running, initialized `qu lsp`; null means "use the fallback" */
let lsp = null;
/** @type {vscode.Disposable[]} the providers registered against `lsp` */
let lspProviders = [];
/** @type {Set<string>} URIs the server has been sent didOpen for */
const lspOpen = new Set();
/** Bumped by every (re)start, so a start that lost a race can tell. */
let lspGeneration = 0;

function activate(context) {
    extensionPath = context.extensionPath;
    outputChannel = vscode.window.createOutputChannel('Qu');
    diagnosticCollection = vscode.languages.createDiagnosticCollection('qu');
    runDiagnosticCollection = vscode.languages.createDiagnosticCollection('qu-run');
    context.subscriptions.push(outputChannel, diagnosticCollection, runDiagnosticCollection);

    context.subscriptions.push(vscode.commands.registerCommand('qu.runFile', runFile));
    context.subscriptions.push(vscode.commands.registerCommand('qu.htmlDiff', htmlDiffWithFile));

    context.subscriptions.push(
        vscode.workspace.onDidOpenTextDocument((doc) => {
            lspDidOpen(doc);
            scheduleCheck(doc);
        }),
        vscode.workspace.onDidChangeTextDocument((e) => {
            lspDidChange(e.document);
            scheduleCheck(e.document);
        }),
        vscode.workspace.onDidSaveTextDocument(scheduleCheck),
        vscode.workspace.onDidChangeConfiguration((e) => {
            if (e.affectsConfiguration('qu.executablePath') || e.affectsConfiguration('qu.languageServer.enable')) {
                startLanguageServer();
            }
        }),
        vscode.workspace.onDidCloseTextDocument((doc) => {
            lspDidClose(doc);
            diagnosticCollection.delete(doc.uri);
            const key = doc.uri.toString();
            clearTimeout(debounceTimers.get(key));
            debounceTimers.delete(key);
        })
    );

    // Any .qu files already open when the extension activates should get
    // diagnostics immediately, not just on the next edit. (These fallback
    // checks are dropped again if the language server comes up first.)
    vscode.workspace.textDocuments.forEach(scheduleCheck);

    // Returned so a caller (VS Code ignores it; the tests await it) can tell
    // when the language server is up or has been given up on.
    return startLanguageServer();
}

function deactivate() {
    debounceTimers.forEach((t) => clearTimeout(t));
    debounceTimers.clear();
    return stopLanguageServer();
}

// ------------------------------------------------------------------ language server

/**
 * Start `qu lsp` and register the providers it backs. Never throws and
 * never shows a pop-up: a `qu` without `lsp` (older than the release that
 * added it) is a normal situation, reported as one line in the Qu output
 * channel, after which the `qu parse --json` diagnostics keep working as
 * they did before the server existed.
 */
async function startLanguageServer() {
    await stopLanguageServer();
    const generation = ++lspGeneration;
    if (!vscode.workspace.getConfiguration('qu').get('languageServer.enable', true)) return;
    const quPath = resolveQuExecutable();
    if (!quPath) return; // the run/diagnostics paths already report a missing qu

    const client = new QuLspClient(quPath);
    client.on('stderr', (text) => outputChannel.append(`[qu lsp] ${text}`));
    client.on('error', (err) => outputChannel.appendLine(`[qu lsp: ${err.message}]`));
    client.on('notification', (method, params) => {
        if (method === 'textDocument/publishDiagnostics' && lsp === client) applyServerDiagnostics(params);
    });
    client.on('exit', (code) => {
        if (lsp !== client) return; // stopped on purpose, or never became current
        lsp = null;
        disposeLspProviders();
        lspOpen.clear();
        outputChannel.appendLine(
            `[qu lsp stopped (exit code ${code}); hover, completion and the outline are off until the window is reloaded. Diagnostics continue through qu parse --json.]`
        );
        vscode.workspace.textDocuments.forEach(scheduleCheck);
    });

    try {
        await client.start({
            textDocument: {
                synchronization: { didSave: false },
                publishDiagnostics: { versionSupport: true },
                hover: { contentFormat: ['markdown', 'plaintext'] },
                completion: { completionItem: { documentationFormat: ['markdown', 'plaintext'] } },
                signatureHelp: {
                    signatureInformation: {
                        documentationFormat: ['markdown', 'plaintext'],
                        parameterInformation: { labelOffsetSupport: true },
                    },
                },
                documentSymbol: { hierarchicalDocumentSymbolSupport: true },
                definition: {},
            },
        });
    } catch (err) {
        outputChannel.appendLine(
            `[qu lsp did not start (${err.message}) using "${quPath}". Hover, completion and the outline need a qu that has the lsp command; diagnostics continue through qu parse --json.]`
        );
        return;
    }
    if (generation !== lspGeneration) {
        client.stop(); // a newer start (settings changed meanwhile) owns the session
        return;
    }
    lsp = client;
    // The server publishes diagnostics from here on: drop pending fallback checks.
    debounceTimers.forEach((t) => clearTimeout(t));
    debounceTimers.clear();
    registerLspProviders();
    vscode.workspace.textDocuments.forEach(lspDidOpen);
}

async function stopLanguageServer() {
    const client = lsp;
    lsp = null;
    disposeLspProviders();
    lspOpen.clear();
    if (client) await client.stop();
}

function disposeLspProviders() {
    lspProviders.forEach((d) => d.dispose());
    lspProviders = [];
}

function lspDidOpen(document) {
    if (!lsp || !document || document.languageId !== 'qu') return;
    const uri = document.uri.toString();
    if (lspOpen.has(uri)) return;
    lspOpen.add(uri);
    lsp.notify('textDocument/didOpen', {
        textDocument: { uri, languageId: 'qu', version: document.version, text: document.getText() },
    });
}

function lspDidChange(document) {
    if (!lsp || !document || document.languageId !== 'qu') return;
    const uri = document.uri.toString();
    if (!lspOpen.has(uri)) {
        lspDidOpen(document);
        return;
    }
    // The server asked for full-text sync (TextDocumentSyncKind.Full).
    lsp.notify('textDocument/didChange', {
        textDocument: { uri, version: document.version },
        contentChanges: [{ text: document.getText() }],
    });
}

function lspDidClose(document) {
    if (!lsp || !document) return;
    const uri = document.uri.toString();
    if (!lspOpen.delete(uri)) return;
    lsp.notify('textDocument/didClose', { textDocument: { uri } });
}

function toRange(r) {
    return new vscode.Range(r.start.line, r.start.character, r.end.line, r.end.character);
}

function toMarkdown(content) {
    if (!content) return undefined;
    if (typeof content === 'string') return content;
    return content.kind === 'markdown' ? new vscode.MarkdownString(content.value) : content.value;
}

function applyServerDiagnostics(params) {
    const uri = vscode.Uri.parse(params.uri);
    if (!vscode.workspace.getConfiguration('qu').get('diagnostics.enable', true)) {
        diagnosticCollection.delete(uri);
        return;
    }
    const diagnostics = (params.diagnostics || []).map((d) => {
        // LSP severities are 1..4 (Error..Hint), VS Code's are 0..3.
        const out = new vscode.Diagnostic(toRange(d.range), d.message, Math.max(0, (d.severity || 1) - 1));
        out.source = d.source || 'qu';
        return out;
    });
    diagnosticCollection.set(uri, diagnostics);
}

function registerLspProviders() {
    const selector = { language: 'qu' };
    const caps = (lsp && lsp.capabilities) || {};
    const at = (document, position) => ({
        textDocument: { uri: document.uri.toString() },
        position: { line: position.line, character: position.character },
    });
    // A failed or orphaned request (the server stopped) is "no result",
    // never an error surfaced in the editor.
    const ask = (method, params) => (lsp ? lsp.request(method, params).catch(() => null) : Promise.resolve(null));

    if (caps.hoverProvider) {
        lspProviders.push(
            vscode.languages.registerHoverProvider(selector, {
                async provideHover(document, position) {
                    const r = await ask('textDocument/hover', at(document, position));
                    if (!r || !r.contents) return undefined;
                    return new vscode.Hover(toMarkdown(r.contents), r.range ? toRange(r.range) : undefined);
                },
            })
        );
    }

    if (caps.completionProvider) {
        lspProviders.push(
            vscode.languages.registerCompletionItemProvider(
                selector,
                {
                    async provideCompletionItems(document, position) {
                        const r = await ask('textDocument/completion', at(document, position));
                        if (!r) return undefined;
                        const list = Array.isArray(r) ? { isIncomplete: false, items: r } : r;
                        const items = (list.items || []).map((i) => {
                            // CompletionItemKind: LSP is 1-based, VS Code 0-based, same order.
                            const item = new vscode.CompletionItem(i.label, i.kind ? i.kind - 1 : undefined);
                            if (i.detail) item.detail = i.detail;
                            if (i.documentation) item.documentation = toMarkdown(i.documentation);
                            if (i.sortText) item.sortText = i.sortText;
                            if (i.insertText) item.insertText = i.insertText;
                            return item;
                        });
                        return new vscode.CompletionList(items, !!list.isIncomplete);
                    },
                },
                // VS Code already asks on every word character; `.` is for `module.member`.
                '.'
            )
        );
    }

    if (caps.signatureHelpProvider) {
        const triggers = caps.signatureHelpProvider.triggerCharacters || ['(', ','];
        lspProviders.push(
            vscode.languages.registerSignatureHelpProvider(
                selector,
                {
                    async provideSignatureHelp(document, position) {
                        const r = await ask('textDocument/signatureHelp', at(document, position));
                        if (!r || !r.signatures || !r.signatures.length) return undefined;
                        const help = new vscode.SignatureHelp();
                        help.signatures = r.signatures.map((s) => {
                            const info = new vscode.SignatureInformation(s.label, toMarkdown(s.documentation));
                            info.parameters = (s.parameters || []).map(
                                (p) => new vscode.ParameterInformation(p.label, toMarkdown(p.documentation))
                            );
                            return info;
                        });
                        help.activeSignature = r.activeSignature || 0;
                        help.activeParameter = r.activeParameter || 0;
                        return help;
                    },
                },
                ...triggers
            )
        );
    }

    if (caps.documentSymbolProvider) {
        // SymbolKind: LSP is 1-based, VS Code 0-based, same order.
        const toSymbol = (s) => {
            const out = new vscode.DocumentSymbol(
                s.name,
                s.detail || '',
                s.kind - 1,
                toRange(s.range),
                toRange(s.selectionRange)
            );
            out.children = (s.children || []).map(toSymbol);
            return out;
        };
        lspProviders.push(
            vscode.languages.registerDocumentSymbolProvider(selector, {
                async provideDocumentSymbols(document) {
                    const r = await ask('textDocument/documentSymbol', {
                        textDocument: { uri: document.uri.toString() },
                    });
                    return Array.isArray(r) ? r.map(toSymbol) : undefined;
                },
            })
        );
    }

    if (caps.definitionProvider) {
        lspProviders.push(
            vscode.languages.registerDefinitionProvider(selector, {
                async provideDefinition(document, position) {
                    const r = await ask('textDocument/definition', at(document, position));
                    if (!r) return undefined;
                    const toLocation = (l) => new vscode.Location(vscode.Uri.parse(l.uri), toRange(l.range));
                    return Array.isArray(r) ? r.map(toLocation) : toLocation(r);
                },
            })
        );
    }
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

    runDiagnosticCollection.clear();
    let captured = '';
    const onData = (d) => {
        const s = d.toString();
        captured += s;
        outputChannel.append(s);
    };
    child.stdout.on('data', onData);
    child.stderr.on('data', onData);
    child.on('error', (err) => {
        outputChannel.appendLine(`[qu run failed to start: ${err.message}]`);
        reportMissingExecutable();
    });
    child.on('close', (code) => {
        outputChannel.appendLine(`--- qu exited with code ${code} ---`);
        if (code !== 0) publishTraceDiagnostics(captured, filePath);
        appendVarsDumpIfPresent(varsPath);
    });
}

/**
 * `qu run` prints, under a runtime error,
 *     at file.qu:12 in f()
 *     called from file.qu:30 (top level)
 * Turn those lines into Problems-panel entries: the `at` frame is an error
 * carrying the error message, each `called from` frame an information entry.
 * Only frames in the script that was run are mapped (a frame in an imported
 * module names that module's line under the main script's name).
 */
function parseTrace(text) {
    const lines = text.split(/\r?\n/);
    const frames = [];
    let message = '';
    const re = /^\s+(at|called from)\s+(.+?):(\d+)(?:\s+in\s+(\S+\(\))|\s+\(top level\))?\s*$/;
    for (const line of lines) {
        const m = re.exec(line);
        if (m) {
            frames.push({ kind: m[1], file: m[2], line: parseInt(m[3], 10), fn: m[4] || '' });
        } else if (frames.length === 0 && line.trim()) {
            message = line.trim().replace(/^qu:\s*/, '');
        }
    }
    return { message, frames };
}

function publishTraceDiagnostics(text, scriptPath) {
    const { message, frames } = parseTrace(text);
    if (frames.length === 0) return;
    const uri = vscode.Uri.file(scriptPath);
    const base = path.basename(scriptPath);
    const diagnostics = [];
    for (const f of frames) {
        if (path.basename(f.file) !== base) continue;
        const line = Math.max(0, f.line - 1);
        const range = new vscode.Range(line, 0, line, Number.MAX_SAFE_INTEGER);
        const isAt = f.kind === 'at';
        const d = new vscode.Diagnostic(
            range,
            isAt ? message || 'runtime error' : `called from here${f.fn ? ` (in ${f.fn})` : ''}`,
            isAt ? vscode.DiagnosticSeverity.Error : vscode.DiagnosticSeverity.Information
        );
        d.source = 'qu run';
        diagnostics.push(d);
    }
    if (diagnostics.length) runDiagnosticCollection.set(uri, diagnostics);
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
        // `--emit-vars` also lists the built-in constants (pi, e, QuTab, ...)
        // flagged `system: true`; the dump is about the script's own variables.
        if (Array.isArray(vars)) vars = vars.filter((v) => !v.system);
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

// ------------------------------------------------------------------ html diff

/**
 * `qu.htmlDiff`. Saves the active document if dirty, prompts for a second
 * file to compare against, then runs the bundled `qu_htmldiff.qu` (via
 * QU_DIFF_OLD/QU_DIFF_NEW/QU_DIFF_OUT env vars — that script takes no
 * positional args, matching this project's `qu run` convention) and shows
 * the resulting side-by-side HTML diff in a webview panel beside the editor.
 *
 * `contextUri` is the resource VS Code passes when this command is invoked
 * from the Explorer's right-click menu — the clicked file, not necessarily
 * whatever's active in the editor. Falls back to the active editor's
 * document for the Command Palette / keybinding invocation, where no such
 * argument is passed.
 */
async function htmlDiffWithFile(contextUri) {
    const quPath = resolveQuExecutable();
    if (!quPath) {
        reportMissingExecutable();
        return;
    }

    let oldPath;
    if (contextUri instanceof vscode.Uri) {
        oldPath = contextUri.fsPath;
        const openDoc = vscode.workspace.textDocuments.find((d) => d.uri.fsPath === oldPath);
        if (openDoc && openDoc.isDirty) {
            await openDoc.save();
        }
    } else {
        const editor = vscode.window.activeTextEditor;
        if (!editor) {
            vscode.window.showWarningMessage('Qu: HTML Diff needs an open file to compare from.');
            return;
        }
        if (editor.document.isDirty) {
            await editor.document.save();
        }
        oldPath = editor.document.fileName;
    }

    const picked = await vscode.window.showOpenDialog({
        canSelectMany: false,
        openLabel: 'Compare With This File',
        title: `Qu: HTML Diff — choose the file to compare "${path.basename(oldPath)}" against`,
    });
    if (!picked || picked.length === 0) return; // user cancelled
    const newPath = picked[0].fsPath;

    const scriptPath = path.join(extensionPath, 'scripts', 'qu_htmldiff.qu');
    const outPath = path.join(
        os.tmpdir(),
        `qu-vscode-htmldiff-${process.pid}-${Date.now()}.html`
    );

    outputChannel.appendLine(`--- qu html diff "${oldPath}" vs "${newPath}" ---`);

    const env = Object.assign({}, process.env, {
        QU_DIFF_OLD: oldPath,
        QU_DIFF_NEW: newPath,
        QU_DIFF_OUT: outPath,
    });

    cp.execFile(quPath, ['run', scriptPath], { env, timeout: 30000 }, (error, stdout, stderr) => {
        if (stdout) outputChannel.append(stdout);
        if (stderr) outputChannel.append(stderr);
        if (error) {
            vscode.window.showErrorMessage(`Qu: HTML diff failed — see the "Qu" output channel.`);
            outputChannel.show(true);
            return;
        }
        fs.readFile(outPath, 'utf8', (readErr, html) => {
            fs.unlink(outPath, () => {}); // best-effort cleanup, ignore failures
            if (readErr) {
                vscode.window.showErrorMessage(`Qu: HTML diff produced no output — see the "Qu" output channel.`);
                outputChannel.show(true);
                return;
            }
            const panel = vscode.window.createWebviewPanel(
                'quHtmlDiff',
                `Diff: ${path.basename(oldPath)} vs ${path.basename(newPath)}`,
                vscode.ViewColumn.Beside,
                { enableScripts: true } // the diff page's own "show changed lines only" checkbox needs this
            );
            panel.webview.html = html;
        });
    });
}

// ------------------------------------------------------------------ diagnostics

function scheduleCheck(document) {
    if (!document || document.languageId !== 'qu') return;
    // With the language server running, diagnostics come from it (see
    // applyServerDiagnostics); this `qu parse --json` path is the fallback.
    if (lsp) return;
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
    if (!quPath) {
        reportMissingExecutable(); // shows its message once per session, not on every keystroke
        return;
    }

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
    const configured = (vscode.workspace.getConfiguration('qu').get('executablePath') || '').trim() || 'qu';
    // A bare name ("qu", the default) is looked up on PATH; anything with a
    // path separator is taken literally.
    if (!/[\\/]/.test(configured)) {
        return findOnPath(configured);
    }
    return fs.existsSync(configured) ? configured : null;
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

let warnedMissingExecutable = false;

/**
 * Tell the user, once per session, that `qu` cannot be started and what to do
 * about it. Later failures only add a line to the Output channel, so a missing
 * executable never turns into a stream of pop-ups (or a stack trace).
 */
function reportMissingExecutable() {
    const configured = (vscode.workspace.getConfiguration('qu').get('executablePath') || '').trim() || 'qu';
    const message = /[\\/]/.test(configured)
        ? `Qu: the configured qu.executablePath ("${configured}") does not exist.`
        : `Qu: could not find "${configured}" on PATH. Set qu.executablePath to the full path of qu (qu.exe), or put its folder on PATH. Running "qu editors status" in a terminal shows where qu is.`;
    if (outputChannel) outputChannel.appendLine(`[${message}]`);
    if (warnedMissingExecutable) return;
    warnedMissingExecutable = true;
    vscode.window.showErrorMessage(message, 'Locate qu...', 'Open Settings').then(async (choice) => {
        if (choice === 'Open Settings') {
            vscode.commands.executeCommand('workbench.action.openSettings', 'qu.executablePath');
        } else if (choice === 'Locate qu...') {
            const picked = await vscode.window.showOpenDialog({
                canSelectMany: false,
                openLabel: 'Use this qu',
                title: 'Select the qu executable (qu.exe)',
            });
            if (picked && picked.length) {
                await vscode.workspace
                    .getConfiguration('qu')
                    .update('executablePath', picked[0].fsPath, vscode.ConfigurationTarget.Global);
                warnedMissingExecutable = false;
                vscode.workspace.textDocuments.forEach(scheduleCheck);
            }
        }
    });
}

module.exports = { activate, deactivate };
