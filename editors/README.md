# Editor integrations for Qu

`qu editors` (built into the `qu` binary, which embeds every file here)
detects, installs, checks and removes these:

| Editor | Folder | Installed to |
|---|---|---|
| VS Code, VSCodium, Cursor, Windsurf | `vscode-qu/` | `<home>/.vscode\|.vscode-oss\|.cursor\|.windsurf/extensions/qu-project.qu-language-<ver>/` (portable: `data/extensions`; Flatpak: `~/.var/app/...`) |
| Notepad++ (Windows) | `notepadpp-qu/` | `%APPDATA%\Notepad++\userDefineLangs\Qu.udl.xml` (portable: next to `notepad++.exe`) |
| Sublime Text 3 and 4 | `sublime-qu/` | `Packages/User/Qu/` |
| Vim | `vim-qu/` | `~/.vim/pack/qu/start/qu/` (Windows `vimfiles`) |
| Neovim | `vim-qu/` | `~/.config/nvim/pack/qu/start/qu/` (Windows `%LOCALAPPDATA%\nvim`) |
| Jupyter (JupyterLab, Notebook, VS Code notebooks) | `engine/crates/qu-jupyter` | kernelspec `qu` via the sibling `qu-jupyter` binary |

```
qu editors detect [<editor>] [--json]     where each editor is and where its plugin goes (exit 0/1 with a name)
qu editors status [--json]                installed? version? files intact? qu on PATH? does the CLI list it?
qu editors install [--editor auto|all|NAME[,NAME]] [--root DIR] [--dry-run]
qu editors uninstall [--editor ...] [--root DIR] [--dry-run]
qu editors check [--json]                 exit 0 only if everything installed is healthy
```

Detection order: Windows registry (`App Paths`, then the `Uninstall` keys, both
registry views, through `reg.exe`), `PATH`, known folders (Program Files,
`%LOCALAPPDATA%\Programs`); macOS `/Applications` and `~/Applications`; Linux
`PATH`, `/usr`, `/opt`, `/snap`, Flatpak. Each result says which method found
it. Editors in custom folders are found through the registry or `PATH`.

`--editor auto` (the default for `install`) installs into every detected
editor; `all` writes to every known default location even if the editor is
not detected. Safe to rerun; user-modified files are saved as `.bak`.

## Language server: `qu lsp`

`qu lsp` is a Language Server Protocol server built into the `qu` binary
(`engine/crates/qu-cli/src/lsp.rs`). Any editor with an LSP client gets the
same answers from it, computed by the same lexer, parser and builtin table
`qu run` uses:

| Feature | LSP method | What it returns |
|---|---|---|
| Diagnostics | `textDocument/publishDiagnostics` | Lexer and parser errors, on open and on every change. The parser stops at its first error, so there is usually one. |
| Hover | `textDocument/hover` | A builtin's signature and one-line summary; a user function's header line and the comment block above it. |
| Completion | `textDocument/completion` | Builtins (signature and summary attached), the file's own functions and variables, keywords; after `module.` the members of that module. Nothing inside comments and strings. |
| Signature help | `textDocument/signatureHelp` | The documented parameter list, with the active parameter found by counting commas (or by a `name=` argument). |
| Outline | `textDocument/documentSymbol` | Functions, `#%%` cells and `# --- title` sections, nested cell > section > function. |
| Go to definition | `textDocument/definition` | A user function, or a variable's first assignment, in the same file. |

What a client needs to know, all of it from the server's `initialize` reply:

- **Transport**: stdio. The command is `qu lsp` (`qu lsp --stdio` is accepted
  and means the same). No TCP or pipe transport.
- **Document sync**: full text (`TextDocumentSyncKind.Full`), with
  `didOpen`/`didChange`/`didClose`.
- **Positions**: UTF-16 code units, the protocol default. The server does not
  negotiate another encoding.
- **Language id / file pattern**: the server does not look at the language id;
  the client must be told to start it for `*.qu` files.
- **Not implemented**: references, rename, formatting (use `qu fmt`), workspace
  symbols, semantic tokens, anything across files. A request for one of these
  gets a `MethodNotFound` error reply, as the protocol specifies.

`qu` must be on `PATH` for the configurations below, or replace `"qu"` with
the full path (`qu editors status` prints it).

### Which of these were actually run

| Client | Status |
|---|---|
| The server itself | Tested: `engine/crates/qu-cli/tests/lsp.rs` spawns `qu lsp` and speaks framed JSON-RPC to it; `src/lsp.rs` has unit tests for each handler. |
| VS Code / VSCodium / Cursor / Windsurf | The extension's client (`vscode-qu/lspClient.js`) is tested against a real `qu lsp` with plain node, and the extension's glue is tested with a stub of the `vscode` API. `vscode-qu/test/vscode_host_test.js` was also run once inside a real extension host (VSCodium 1.107.1, 2026-11-11, throw-away profile): diagnostics, hover, completion, signature help, document symbols and definition all answered through VS Code's own provider commands. That run is manual, not part of `cargo test`, and nobody looked at the rendered pop-ups. VS Code, Cursor and Windsurf themselves were not run. |
| Neovim, Helix, Sublime LSP, Kate, JetBrains (LSP4IJ) | **Not tested in the editor.** The configurations below are written from each client's documented configuration format and the facts above; none of these editors was run. |

### VS Code, VSCodium, Cursor, Windsurf

Nothing to configure: the extension (`qu editors install`) starts `qu lsp`
itself, using the `qu.executablePath` setting. Setting
`qu.languageServer.enable` to `false` turns it off. With a `qu` too old to
have `lsp`, the extension logs one line to the Qu output channel and keeps its
previous `qu parse --json` diagnostics. The client is a small one written for
this extension rather than `vscode-languageclient`: that package and its
dependencies are several hundred files, and this extension is installed by
copying a handful of files with no build step.

### Neovim (0.8 or later, no plugin)

The `vim-qu` plugin sets the `qu` filetype. In `init.lua`:

```lua
vim.api.nvim_create_autocmd('FileType', {
  pattern = 'qu',
  callback = function(args)
    vim.lsp.start({
      name = 'qu-lsp',
      cmd = { 'qu', 'lsp' },
      -- One server per directory of .qu files; the server itself is single-file.
      root_dir = vim.fs.dirname(vim.api.nvim_buf_get_name(args.buf)),
    })
  end,
})
```

`vim.lsp.start` reuses a running client with the same `name` and `root_dir`.
Diagnostics appear on their own. Hover, definition and completion use Neovim's
standard LSP entry points (`vim.lsp.buf.hover()`, `vim.lsp.buf.definition()`,
`vim.lsp.buf.signature_help()`, `vim.lsp.buf.document_symbol()`, and omni
completion through `vim.lsp.omnifunc`); which keys those are on by default
depends on the Neovim version.

### Helix

In `~/.config/helix/languages.toml` (Windows: `%AppData%\helix\languages.toml`):

```toml
[language-server.qu-lsp]
command = "qu"
args = ["lsp"]

[[language]]
name = "qu"
scope = "source.qu"
file-types = ["qu"]
comment-token = "#"
roots = []
language-servers = ["qu-lsp"]
```

This gives Helix the language server only. There is no tree-sitter grammar for
Qu, so there is no syntax highlighting in Helix. `hx --health qu` reports
whether Helix finds the `qu` binary.

### Sublime Text (LSP package)

Install the `LSP` package from Package Control, then
Preferences > Package Settings > LSP > Settings:

```json
{
  "clients": {
    "qu": {
      "enabled": true,
      "command": ["qu", "lsp"],
      "selector": "source.qu"
    }
  }
}
```

`source.qu` is the scope of `sublime-qu/Qu.sublime-syntax`, so that syntax
(`qu editors install --editor sublime`) must be installed for the selector to
match.

### Kate

Settings > Configure Kate > LSP Client > User Server Settings:

```json
{
  "servers": {
    "qu": {
      "command": ["qu", "lsp"],
      "highlightingModeRegex": "^Qu$"
    }
  }
}
```

Kate chooses a server by the document's highlighting mode, and Qu has no
KSyntaxHighlighting definition yet (see below). Until one exists, or you
select a mode named `Qu` by other means, Kate will not start the server for
a `.qu` file. The block above is the server half only.

### JetBrains IDEs (LSP4IJ plugin)

Install the LSP4IJ plugin, then Settings > Languages & Frameworks >
Language Servers > `+` (new language server):

- **Name**: `Qu`
- **Command**: `qu lsp`
- **Mappings > File name patterns**: `*.qu`, language id `qu`

Highlighting is separate: import `vscode-qu/syntaxes/qu.tmLanguage.json` as a
TextMate bundle (see below).

### Any other client

Start `qu lsp` with stdio transport for files matching `*.qu`. Nothing else is
required: there are no initialization options and no workspace configuration.

## Not yet supported

| Editor | Why |
|---|---|
| JetBrains IDEs | They can import the TextMate grammar `vscode-qu/syntaxes/qu.tmLanguage.json` as a TextMate bundle (Settings > Editor > TextMate Bundles); no installer yet. The language server works through LSP4IJ (above). |
| Zed, Helix, Neovim tree-sitter | Highlighting needs a tree-sitter grammar for Qu, which does not exist yet. Helix can use the language server without one (above). Zed needs an extension to register a language, which does not exist. |
| Emacs | Needs a `qu-mode.el`; with one, `eglot` or `lsp-mode` can be pointed at `qu lsp`. |
| Kate | Needs a KSyntaxHighlighting XML definition, also for the language server to be selected (above). |
