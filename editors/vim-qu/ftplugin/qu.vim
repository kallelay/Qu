" Vim filetype plugin for Qu (.qu)
" Qu editor plugin 0.2.0
if exists("b:did_ftplugin")
  finish
endif
let b:did_ftplugin = 1

setlocal commentstring=#\ %s
setlocal comments=:#
setlocal formatoptions-=t
setlocal formatoptions+=croql
setlocal expandtab
setlocal shiftwidth=4
setlocal softtabstop=4
setlocal tabstop=4

" `%` jumps between block openers and their `end` (needs the bundled matchit
" plugin: `packadd matchit` in Vim; Neovim loads it by default).
let b:match_words = '\<\%(if\|for\|while\|function\|sub\|class\|try\)\>:\<\%(elseif\|else\|catch\)\>:\<end\>,\<repeat\>:\<until\>'
let b:match_skip = 's:quComment\|quString\|quRawString'

let b:undo_ftplugin = "setlocal commentstring< comments< formatoptions< expandtab< shiftwidth< softtabstop< tabstop<"
  \ . " | unlet! b:match_words b:match_skip"
