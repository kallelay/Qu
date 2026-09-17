# Qu Language (Notepad++)

A Notepad++ User Defined Language (UDL) definition for Qu (`.qu` files):
keyword groups (control-flow, domain vocabulary, types, booleans), `#` line
comments, double/single-quoted strings with backslash escapes, and operators.

Notepad++'s UDL engine has no nested/embedded scopes, so unlike the VS Code
and Sublime Text grammars in `editors/`, this definition cannot color
`{expr}` string interpolation differently from the rest of the string — the
whole string, interpolation included, is styled as one block.

## Import

1. Open Notepad++.
2. Menu: **Language -> User Defined Language -> Import...**
3. Select `Qu.udl.xml` from this folder.
4. Restart Notepad++ (UDLs are loaded at startup).
5. Open a `.qu` file, or set the language manually via
   **Language -> Qu** (it will appear near the bottom of the Language menu,
   under user-defined languages).

## Running a file

Vanilla Notepad++ has no build-system concept and no "run" button of its
own — anything beyond syntax highlighting requires either its built-in
**Run...** dialog (works out of the box, one-time 30-second setup, output
opens in its own console window) or the separate **NppExec** plugin (a
console panel docked inside Notepad++, closer to what VS Code/Sublime give
you, but requires installing a plugin first). Both are documented below;
neither is fabricated — this is genuinely what's available.

### Option A: the built-in Run dialog (no plugin needed)

1. Menu: **Run > Run...** (or press **F5**).
2. Enter this command (Windows; the `cmd /k` wrapper keeps the console
   window open after `qu` exits so you can actually read the output —
   without it the window flashes and closes immediately):
   ```
   cmd /k qu run "$(FULL_CURRENT_PATH)"
   ```
   If `qu.exe` isn't on your `PATH`, use its full path instead of bare `qu`,
   e.g. `cmd /k "C:\path\to\qu.exe" run "$(FULL_CURRENT_PATH)"`.
3. Click **Save...** to add it as a permanent entry (e.g. name it "Run Qu
   File") with a keyboard shortcut of your choice — it then appears under
   **Run** for one-click reuse, without retyping the command each time.

This opens a real console window per run; closing it (or typing `exit`)
gets rid of it.

### Option B: NppExec plugin (integrated console panel)

1. Install NppExec: **Plugins > Plugins Admin...**, search for "NppExec",
   install, restart Notepad++.
2. **Plugins > NppExec > Execute...** (or **F6**), and enter:
   ```
   cd "$(CURRENT_DIRECTORY)"
   qu run "$(FULL_CURRENT_PATH)"
   ```
   (again, substitute `qu`'s full path if it isn't on `PATH`).
3. Output appears directly in NppExec's own console panel inside the
   Notepad++ window — no separate window, and the panel stays open between
   runs.
4. Optionally **Save...** the script (e.g. "Run Qu File") and use
   **Plugins > NppExec > Advanced Options** to add it as a permanent menu
   item or toolbar button bound to **F6**-style one-key reuse.

## Debugging

There is no debugger here, and none is being simulated. Neither Notepad++'s
built-in Run dialog nor the NppExec plugin provide anything resembling
breakpoints, stepping, or live variable inspection for an arbitrary
external interpreter — and even if they did, Qu's own execution model is
`qu run <file>`: a single one-shot subprocess with no persistent
interpreter state, no stepping protocol, and no way to pause/inspect/resume
mid-execution. Real debugging support would require engine-level work (an
interactive/steppable execution mode) before any editor integration could
be built on top of it.

## Keeping this in sync

The keyword groups here are meant to track
`qu-ui-components/src/components/CodeEditor.tsx`'s `QU_LANGUAGE_CONFIG`
(the Monaco grammar used by Qu Studio), plus `layer` and `enum`, which are
real keywords (see `docs/qu-grammar.ebnf` and
`engine/crates/qu-syntax/src/lib.rs`) not yet present in that list. If that
canonical config changes, update this file, `editors/vscode-qu/syntaxes/qu.tmLanguage.json`,
and `editors/sublime-qu/Qu.sublime-syntax` to match.
