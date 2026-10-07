# Qu for Vim and Neovim

Filetype detection (`*.qu`), syntax highlighting and a filetype plugin
(`commentstring=# `, 4-space indent, `%` matching for `if/for/while/function/
sub/class/try ... end` and `repeat ... until`) for Vim 8+ and Neovim.

Highlighted: keywords, constants (`true false none`), type words, numbers
(hex, exponent, `i`/`j`, attached units such as `5mV`), strings with escapes
and `{interpolation}`, raw strings `r"..."` and `r"""..."""`, `#` comments and
`#%%` cell markers. `A'` after a name, number or closing bracket is the
transpose operator and is not mistaken for a string.

**Deliberately omitted: highlighting of the ~1200 builtin function names.**
That list changes every release; a hand-copied list would be wrong within a
month. The keyword and unit lists in `syntax/qu.vim` are checked against the
lexer (`qu_lexer::KEYWORDS`, `qu_lexer::UNITS`) by `qu editors check` and by
the unit tests, so they cannot drift silently.

## Install

```sh
qu editors install --editor vim      # ~/.vim            (Windows: %USERPROFILE%\vimfiles)
qu editors install --editor nvim     # ~/.config/nvim    (Windows: %LOCALAPPDATA%\nvim)
```

Both use Vim's native packages layout, `pack/qu/start/qu/{syntax,ftdetect,
ftplugin}`, so no plugin manager is needed. `qu editors uninstall --editor vim`
removes exactly those files (and the then-empty `pack/qu/...` folders).

By hand: copy this folder's `syntax/`, `ftdetect/` and `ftplugin/` into
`~/.vim/pack/qu/start/qu/` (Neovim: `~/.config/nvim/pack/qu/start/qu/`).

For `%` matching in classic Vim add `packadd matchit` to your vimrc (Neovim
loads it by default).
