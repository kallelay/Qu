# Changelog (Qu editor plugins)

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
