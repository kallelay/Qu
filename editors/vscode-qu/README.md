# Qu Language (VS Code)

Syntax highlighting, a run command, and live parse diagnostics for the Qu
scientific scripting language (`.qu` files): keywords, types, operators,
numbers (including hex, exponent, and `i`/`j` imaginary literals),
single/double-quoted strings with `{expr}` interpolation highlighted
distinctly, and `#` line comments — plus running the current file and
seeing real syntax-error squiggles as you type.

This extension is not published to the Marketplace. The same extension also
works in **VSCodium, Cursor and Windsurf** (they are VS Code forks that read
the same extension folder layout).

## Option 0: `qu editors install` (recommended, 0.2.0 and later)

The `qu` binary carries a copy of this extension and installs it for you:

```sh
qu editors detect              # which editors qu finds, and where their plugins go
qu editors install             # install into every detected editor (default: --editor auto)
qu editors install --editor vscode,cursor     # only these
qu editors status              # installed? which version? does `code --list-extensions` list it? is qu on PATH?
qu editors check               # validate the files, compare keywords with the lexer, exit 0 only if healthy
qu editors uninstall           # remove only what install wrote
```

It writes `<extensions dir>/qu-project.qu-language-<version>/`, registers it in
`extensions.json` when that file exists (backed up once as `extensions.json.bak`),
removes older `qu-language-*` copies (such as a hand-copied `qu-language-0.1.0`),
and never overwrites a file you edited without first saving it as `<file>.bak`.
`--dry-run` prints what would happen; `--root DIR` pretends DIR is the whole
machine (used by the tests). Restart the editor afterwards.

## Option A: copy into the VS Code extensions folder (no `qu` needed)

1. Copy this whole `vscode-qu` folder into your VS Code extensions directory:
   - Windows: `%USERPROFILE%\.vscode\extensions\qu-project.qu-language-0.2.0`
   - macOS/Linux: `~/.vscode/extensions/qu-project.qu-language-0.2.0`
2. Restart VS Code (or run "Developer: Reload Window" from the Command Palette).
3. Open any `.qu` file — it should be detected automatically. If not, click the
   language mode indicator in the bottom-right status bar and choose "Qu".

## Option B: package with `vsce` and install the `.vsix`

```sh
npm install -g @vscode/vsce
cd editors/vscode-qu
vsce package
code --install-extension qu-language-0.2.0.vsix
```

## Notebooks

Notebooks (`.ipynb`) in VS Code, VSCodium and Cursor need Microsoft's Jupyter
extension (`ms-toolsai.jupyter`) and the Qu kernel (`qu editors install
--editor jupyter`, which registers the kernelspec through `qu-jupyter`).
`qu editors status` says whether the Jupyter extension is installed and prints
the one-line command (`code --install-extension ms-toolsai.jupyter`); it never
installs it for you.

## Running a file

Command Palette → **Qu: Run File** (command id `qu.runFile`), or the
keybinding **Ctrl+Alt+Q** (**Cmd+Alt+Q** on macOS) while a `.qu` file has
focus, or the ▶ run button in the editor title bar. This saves the file if
it has unsaved changes, then runs `qu run --emit-vars <tmp>.json <file>` and
streams stdout/stderr into an Output Channel named **Qu** (View → Output →
pick "Qu" from the dropdown).

If the run keybinding conflicts with something else on your setup, rebind
`qu.runFile` from Keyboard Shortcuts (search "Qu: Run File").

### Finding the `qu` executable

The setting **Qu › Executable Path** (`qu.executablePath`, default `qu`) names
the executable: a bare name is looked up on your `PATH`, a full path such as
`C:\Qu\qu.exe` is used as is. If `qu` cannot be started the extension shows
**one** message per session (not a stack trace, and not on every keystroke)
with "Locate qu..." and "Open Settings" buttons; later attempts only add a line
to the Qu output channel. `qu editors status` prints whether `qu` is on `PATH`
and the exact path to put in the setting when it is not.

### Runtime errors in the Problems panel

`qu run` prints `at file.qu:LINE in f()` and `called from ...` lines under a
runtime error. **Qu: Run File** turns the `at` line into an error entry on that
line (carrying the error message) and each `called from` line in the same file
into an information entry.

## Diagnostics (syntax-error squiggles)

While `qu.diagnostics.enable` is on (the default), the extension re-parses
the active `.qu` file's current in-editor text about 400ms after you stop
typing (also on open/save), using `qu parse --json` (the CLI's structured
diagnostics mode — see `engine/crates/qu-cli/src/main.rs`'s `parse`
command). A parse error shows up as a real squiggle at the reported
line/column plus a Problems-panel entry, the same class of feedback
QuStudio's own built-in editor already gets from its in-process
`check_syntax` command. Turn it off in Settings if you'd rather not shell
out on every edit.

## Debugging

There is no debugger, and this extension does not pretend to have one. Qu's
execution model is `qu run <file>` — a single one-shot subprocess with no
persistent interpreter state, no stepping protocol, and no way to
pause/inspect/resume mid-execution. VS Code's Debug Adapter Protocol needs
the target runtime to support pausing and reporting variable state at a
breakpoint; Qu's interpreter has no such capability today, and adding it
would be engine-level work (an interactive/steppable execution mode), not
something an editor extension can fake convincingly.

As a genuinely useful — but honest — substitute, **Qu: Run File**'s Output
Channel prints a **post-run variable dump**: the script's final top-level
variable bindings (name, type, short preview), taken from `qu run
--emit-vars`, the same data QuStudio's own Variables panel uses. It is
labeled "post-run snapshot, not a live debugger" in the output — it shows
you the end state after the script finishes, not a paused mid-execution
inspection.

## HTML diff

Command Palette → **Qu: HTML Diff With File...** (command id
`qu.htmlDiff`), also on the Explorer's right-click menu for any file. Saves
the file you're diffing from (or the one you right-clicked) if dirty, asks
you to pick a second file, then runs the bundled `scripts/qu_htmldiff.qu` —
a Qu port of this project's own `tools/qu_htmldiff.qu`/`htmldiff.py` — to
build a side-by-side, word-level HTML diff, shown in a webview panel beside
the editor. Not a source-control diff and not limited to `.qu` files; it's a
general two-file comparison tool that happens to be implemented in Qu, per
this project's own "Qu tooling must be Qu" practice.

## What's included

- `package.json` — language contribution (`id: qu`, extension `.qu`), the
  `qu.runFile`/`qu.htmlDiff` commands + keybinding + editor-title button +
  Explorer context menu entry, and the `qu.executablePath`/
  `qu.diagnostics.enable` settings
- `extension.js` — plain CommonJS, no build step: run command, output
  streaming + post-run vars dump, debounced diagnostics, HTML diff webview,
  and `qu` executable resolution (`PATH` search + the `qu.executablePath`
  override)
- `scripts/qu_htmldiff.qu` — bundled copy of `tools/qu_htmldiff.qu`, run by
  `qu.htmlDiff`; keep it in sync with the repo root copy if that script
  changes
- `language-configuration.json` — comments (`#`), bracket pairs, auto-closing
  pairs, basic indent rules
- `syntaxes/qu.tmLanguage.json` — TextMate grammar (`source.qu`) covering
  keywords, types, operators, numbers, strings + interpolation, comments

## Keeping this in sync

The grammars are hand-written, so they drift. `qu editors check` (and the
unit test `drift_every_grammar_covers_every_lexer_keyword`) compares every
grammar's highlighted words with `qu_lexer::KEYWORDS` and its unit list with
`qu_lexer::UNITS`: a lexer keyword that is not highlighted is a failure, and a
highlighted word that is really a builtin function name is reported. When
the language gains a keyword or unit, update this grammar,
`editors/sublime-qu/Qu.sublime-syntax`, `editors/notepadpp-qu/Qu.udl.xml` and
`editors/vim-qu/syntax/qu.vim` together, and bump the plugin version (the
`Qu editor plugin X.Y.Z` marker in each file and `package.json`).

## What 0.2.0 fixed

See `CHANGELOG.md`: raw strings `r"..."` / `r"""..."""`, `#%%` cell markers,
`5mV`-style unit literals, `A'` (transpose) no longer starting a string, the
`elif` indent rule (the keyword is `elseif`), `qu.executablePath`, one-time
missing-`qu` message, run-error traces in the Problems panel.
