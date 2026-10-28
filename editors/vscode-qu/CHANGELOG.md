# Changelog (Qu editor plugins)

## Unreleased

- Grammar (TextMate + Sublime): `.'` is an operator, not the start of a
  string (`y = A.'` used to open a string that swallowed the rest of the
  file and inverted every later string/code colour); single-quoted strings
  no longer show `{...}` as interpolation (the spec says only double-quoted
  strings interpolate); an unclosed `{` in a string ends at the closing quote
  instead of running on; `{x:0.3f}` colours the format spec; LaTeX groups
  (`"$V_{pp}$"`, `"$10^{-3}$"`) are not interpolations.
- Grammar: contextual words (`model`, `method`, `base`, `unit`, `mesh`, `window`,
  `flag`, `pure`, `data`, ...) are coloured as keywords only when an operand
  follows on the same line (`model resnet(x)`, `window "Title"`), not in
  `model = ...`, `if n > window`, `for method in ...`; keyword-named keyword
  arguments (`on="cpu"`, `from = 1`, `step = 2`) are plain names. `end pool`,
  `end test`, `end bench`, ... closers are coloured.
- `language-configuration.json`: `window = 5`, `model = ...`, `table = ...` and
  one-line `if ... end` no longer indent the next line.
- Run File: the post-run variable dump no longer lists the built-in constants
  (`pi`, `e`, `QuTab`, ... flagged `system` by `--emit-vars`).
- Sublime build: `qu run` syntax errors (`qu: parse error at file.qu:L:C: ...`)
  are clickable; the Parse variant's regex captured the line as the file name.
- Notepad++ UDL: the line-comment marker was in the "close" slot
  (`02#`), so `#` comments were not recognised; it is now the open marker
  (`00#`). `_` is a number character (`1_000`); single-quoted strings end at
  end of line, so a transpose `A'` no longer swallows the rest of the file.

## 0.2.2

- Grammar: `.'` is an operator (it started a string and swallowed the rest of the file); common identifiers (`model`, `window`, `base`, `unit`, `mesh`, ...) are coloured as keywords only when used as one; keyword-named arguments (`on=`, `from=`, `to=`, `step=`) are plain names; `end pool`/`end test`/... close their block; single-quoted strings do not interpolate; an unclosed `{` no longer runs past the closing quote; LaTeX groups are not interpolation and `{x:0.3f}` marks its format spec.
- Indent rules ignore assignments and one-line blocks. Run File no longer prints the built-in variables. **Run File is now Ctrl+Alt+R** (it was Ctrl+Alt+Q, which is AltGr+Q = `@` on a German keyboard).

## 0.2.1

- Grammar: builtin function names (`read`, `set`, `fit`, `input`, `error`,
  `warn`, `swap`, `spawn`, `simulate`, `animate`, `circuit`, `compile`,
  `spectrum`, `param`, `signal`, `table`) are no longer coloured as keywords,
  so `read(x)` is a call. The words the parser really treats as contextual
  keywords (`after`, `parallel`, `select`, `type`, `where`) are coloured only
  in their keyword position in the TextMate, Sublime and Vim grammars; the
  Notepad++ UDL cannot anchor a word to a position, so it leaves them plain.

## 0.2.0

- New: `qu editors install|status|check|uninstall|detect` installs and verifies
  this extension (and the Sublime Text, Notepad++, Vim/Neovim plugins and the
  Jupyter kernel) from the `qu` binary itself.
- Grammar: `r"..."` and `r"""..."""` raw strings (no escapes, no `{}`
  interpolation), `#%%` cell markers, number+unit literals (`5mV`, `10kHz`),
  and `A'` is the transpose operator, not the start of a string.
- `language-configuration.json`: indent rules said `elif` (not a Qu keyword);
  the language uses `elseif`. `'` is no longer auto-closed (it is transpose).
- `qu.executablePath` (default `qu`): a bare name is looked up on PATH, a path
  is used as is. If `qu` cannot be started the extension shows one actionable
  message (with "Locate qu..." and "Open Settings") per session instead of an
  error on every keystroke.
- "Qu: Run File" maps the `at file.qu:LINE in f()` / `called from ...` lines of
  a runtime error to Problems-panel entries.
- Keyword lists are checked against `qu_lexer::KEYWORDS` by `qu editors check`.

## 0.1.0

- Syntax highlighting, Run File, live parse diagnostics, HTML diff.
