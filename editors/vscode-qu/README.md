# Qu Language (VS Code)

Syntax highlighting, a run command, and live parse diagnostics for the Qu
scientific scripting language (`.qu` files): keywords, types, operators,
numbers (including hex, exponent, and `i`/`j` imaginary literals),
single/double-quoted strings with `{expr}` interpolation highlighted
distinctly, and `#` line comments — plus running the current file and
seeing real syntax-error squiggles as you type.

This extension is not published to the Marketplace. Install it locally with
one of the two methods below.

## Option A: copy into the VS Code extensions folder (fastest, no build tools)

1. Copy this whole `vscode-qu` folder into your VS Code extensions directory:
   - Windows: `%USERPROFILE%\.vscode\extensions\qu-language-0.1.0`
   - macOS/Linux: `~/.vscode/extensions/qu-language-0.1.0`
2. Restart VS Code (or run "Developer: Reload Window" from the Command Palette).
3. Open any `.qu` file — it should be detected automatically. If not, click the
   language mode indicator in the bottom-right status bar and choose "Qu".

## Option B: package with `vsce` and install the `.vsix`

```sh
npm install -g @vscode/vsce
cd editors/vscode-qu
vsce package
code --install-extension qu-language-0.1.0.vsix
```

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

The extension looks for `qu`/`qu.exe` on your `PATH` first. If it isn't
there, set an explicit path in Settings → **Qu › Executable Path**
(`qu.executablePath`), e.g. `C:\path\to\engine\target\release\qu.exe`. If
neither resolves, running a file shows an error with an "Open Settings"
button rather than a bare spawn failure.

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

## What's included

- `package.json` — language contribution (`id: qu`, extension `.qu`), the
  `qu.runFile` command + keybinding + editor-title button, and the
  `qu.executablePath`/`qu.diagnostics.enable` settings
- `extension.js` — plain CommonJS, no build step: run command, output
  streaming + post-run vars dump, debounced diagnostics, and `qu`
  executable resolution (`PATH` search + the `qu.executablePath` override)
- `language-configuration.json` — comments (`#`), bracket pairs, auto-closing
  pairs, basic indent rules
- `syntaxes/qu.tmLanguage.json` — TextMate grammar (`source.qu`) covering
  keywords, types, operators, numbers, strings + interpolation, comments

## Keeping this in sync

The keyword/type/operator lists here are meant to track
`qu-ui-components/src/components/CodeEditor.tsx`'s `QU_LANGUAGE_CONFIG`
(the Monaco grammar used by Qu Studio), plus `layer` and `enum`, which are
real keywords (see `docs/qu-grammar.ebnf` and
`engine/crates/qu-syntax/src/lib.rs`) not yet present in that list. If that
canonical config changes, update this grammar, `editors/sublime-qu/Qu.sublime-syntax`,
and `editors/notepadpp-qu/Qu.udl.xml` to match.
