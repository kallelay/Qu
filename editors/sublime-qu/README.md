# Qu Language (Sublime Text)

Syntax highlighting plus a build system for the Qu scientific scripting
language (`.qu` files): keywords, types, operators, numbers, strings with
`{expr}` interpolation, and `#` comments — and running the current file
with Ctrl+B.

## Install

1. Open Sublime Text's Packages folder: **Preferences > Browse Packages...**
2. Create a `Qu` folder inside it and copy both `Qu.sublime-syntax` and
   `Qu.sublime-build` from this folder into it.
3. Open a `.qu` file. It should be detected automatically; if not, click the
   syntax indicator in the bottom-right status bar and choose "Qu".

## Running a file

With a `.qu` file open and its syntax set to Qu, press **Ctrl+B** (Sublime's
standard "build" shortcut — Cmd+B on macOS). `Qu.sublime-build` only
activates for files whose syntax scope is `source.qu`, so it will not
interfere with builds for any other file type.

- **Ctrl+B** (default variant): runs `qu run <file>` and shows stdout/stderr
  in Sublime's build output panel.
- **Ctrl+Shift+B**, then pick **"Parse (check syntax only)"**: runs
  `qu parse <file>` instead — reports OK or the first parse error without
  executing anything.

Clicking on a `parse error at LINE:COL: ...` line in the build output panel
jumps to that location in the file (via the build system's `file_regex`).

### If `qu`/`qu.exe` isn't on PATH

`Qu.sublime-build` invokes the bare command `qu`, which only works if it's
already on your shell's `PATH`. If it isn't, either:

- Add a `"path"` entry to `Qu.sublime-build` pointing at the directory
  containing `qu`/`qu.exe`, e.g. on Windows:
  ```json
  "path": "C:\\path\\to\\engine\\target\\release;$PATH"
  ```
  (there's a commented-out example already in the file — uncomment and
  edit it), or
- Replace `"qu"` in each `"cmd"` array with the executable's full path,
  e.g. `"cmd": ["/full/path/to/qu", "run", "$file"]`.

## What's included

- `Qu.sublime-syntax` — Sublime syntax definition (`source.qu`) covering
  keywords, types, operators, numbers, strings + interpolation, comments
- `Qu.sublime-build` — build system: `qu run $file` (Ctrl+B) and a
  `qu parse $file` variant (Ctrl+Shift+B)

## Debugging

Sublime Text has no built-in debugger UI or debug-adapter story of its own
(unlike VS Code), and this package doesn't add one — nor could it
meaningfully, because Qu's execution model is `qu run <file>`: a single
one-shot subprocess with no persistent interpreter state, no stepping
protocol, and no way to pause/inspect/resume mid-execution. That would need
engine-level work (an interactive/steppable execution mode) before any
editor could build real breakpoints/step-through/live variable inspection
on top of it.

## Keeping this in sync

The keyword/type/operator lists in `Qu.sublime-syntax` are meant to track
`qu-ui-components/src/components/CodeEditor.tsx`'s `QU_LANGUAGE_CONFIG`
(the Monaco grammar used by Qu Studio), plus `layer` and `enum`, which are
real keywords (see `docs/qu-grammar.ebnf` and
`engine/crates/qu-syntax/src/lib.rs`) not yet present in that list. If that
canonical config changes, update this grammar, `editors/vscode-qu/syntaxes/qu.tmLanguage.json`,
and `editors/notepadpp-qu/Qu.udl.xml` to match.
