" Vim syntax file for Qu (.qu)
" Qu editor plugin 0.2.0
"
" Deliberately highlights the language itself -- keywords, constants, types,
" numbers (with unit suffixes), strings (including r"..." / r"""...""" raw
" strings and {interpolation}), comments and #%% cell markers -- and NOT the
" ~1200 builtin function names: that list changes every release, and a
" hand-copied copy would be wrong within a month. `qu editors check` verifies
" the keyword and unit lists below against the lexer (qu_lexer::KEYWORDS,
" qu_lexer::UNITS), so they cannot drift silently.

if exists("b:current_syntax")
  finish
endif

syn case match

syn keyword quControl and as break by case cases catch continue do each else elseif elsewhere end for from if in loop mod not on or otherwise repeat return select skip step then to try until use using where while with
syn keyword quKeyword after animate assert async await backend base circuit class collect compile compose const constant data def device dimension distributed ease elemental enum error every export finetune fit flag frame function global hold implements import inherits input inline interface layer let local method mesh model module namespace new node optional override param parallel project property pure read render reserve release restore run scene3d schedule set signal simd simulate sketch spawn spectrum stencil sub swap table train tune type unit unsafe vectorize view warn watch window
syn keyword quType bool int int64 uint uint64 float float64 double complex complex128 string str array vector matrix tensor record list figure frame logical integer
syn keyword quBoolean true false none

" numbers: hex, decimal/float/exponent, imaginary (i/j), then an attached unit
syn match quNumber /\<0[xX][0-9a-fA-F_]\+\>/
syn match quNumber /\<\d[0-9_]*\%(\.\d[0-9_]*\)\=\%([eE][+-]\=\d\+\)\=[ij]\=/
syn match quNumber /\.\d[0-9_]*\%([eE][+-]\=\d\+\)\=[ij]\=\>/
syn match quUnit /\%(\d\|\.\)\@<=\%(Hz\|kHz\|MHz\|GHz\|ms\|us\|ns\|mV\|kV\|mA\|uA\|mOhm\|kOhm\|MOhm\|Ohm\|ohm\|kohm\|Mohm\|uF\|nF\|pF\|mH\|uH\|mW\|kW\|dBm\|dB\|degC\|degF\|deg\|rad\|cycles\|samples\|MiB\|GiB\|KiB\|km\|cm\|mm\|um\|nm\|kWh\|Ws\|As\|J\|C\|V\|A\|F\|H\|W\|s\|k\|M\|G\|m\|u\|p\|a\)\>/

syn match quOperator /\.\*=\|\.\/=\|:=\|==\|!=\|<=\|>=\|+=\|-=\|\*=\|\/=\|\.=\|\^=\|=>\|\.\^\|\*\*\|\.\*\|\.\/\|\.\\\|[|]>\|->\|??\|[-=<>+*\/\\^!&|~@?]/

" strings. A `'` straight after a name, number, `)`, `]`, `}` or `'` is the
" transpose operator (the lexer's rule), not the start of a string.
syn match quEscape /\\\%([abfnrtv\\"']\|x\x\{1,4}\|u\x\{4}\|U\x\{8}\)/ contained
syn region quInterp matchgroup=quInterpDelim start=/{/ end=/}/ contained contains=quNumber,quUnit,quControl,quKeyword,quBoolean,quOperator
syn region quString start=/"/ skip=/\\./ end=/"/ contains=quEscape,quInterp
syn region quString start=/\%(\w\|[)\]}']\)\@<!'/ skip=/\\./ end=/'/ contains=quEscape,quInterp
" raw strings: no escapes, no interpolation. The triple form is defined last
" so it wins over the single-quote form at the same position.
syn region quRawString start=/\<r"/ end=/"/
syn region quRawString start=/\<r"""/ end=/"""/

" comments, and #%% cell markers (defined last so they win over a plain comment)
syn keyword quTodo TODO FIXME XXX NOTE contained
syn match quComment /#.*$/ contains=quTodo
syn match quCell /^\s*#%%.*$/

hi def link quControl Statement
hi def link quKeyword Keyword
hi def link quType Type
hi def link quBoolean Boolean
hi def link quNumber Number
hi def link quUnit Special
hi def link quOperator Operator
hi def link quString String
hi def link quRawString String
hi def link quEscape SpecialChar
hi def link quInterpDelim Delimiter
hi def link quComment Comment
hi def link quCell Title
hi def link quTodo Todo

let b:current_syntax = "qu"
