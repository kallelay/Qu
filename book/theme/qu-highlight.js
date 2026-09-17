// Qu language definition for the highlight.js bundled with mdBook.
//
// Loaded via [output.html] additional-js in book.toml. This file is placed
// by mdBook's template AFTER book-*.js, which already ran hljs.highlightBlock
// on every code block as soon as it loaded (mdBook's HTML is fully static —
// there is no later "DOMContentLoaded" hook to hitch onto). Since "qu" is not
// one of the languages bundled into mdBook's stock highlight.js, that first
// pass throws `Error('Unknown language: "qu"')` from inside hljs.highlight(),
// which aborts the forEach loop in book.js and leaves every ```qu block (and
// any later code block on the same page after it in document order)
// completely unhighlighted.
//
// So this file does two things:
//   1. Registers a real "qu" grammar with highlight.js.
//   2. Re-scans every `code.language-qu` block and highlights it now that
//      "qu" is known. Note the first pass's "unknown language" fallback
//      still tags the block with the "hljs" class (className bookkeeping
//      runs unconditionally, success or not) even though no highlighting
//      spans were produced -- so presence of the "hljs" class can't be used
//      to detect "already handled"; we key off the language-qu class
//      instead and simply re-run highlighting unconditionally. hljs reads
//      from the live textContent each time, so re-running is safe/idempotent.
//
// Keyword/type/operator/string/comment rules are kept in sync with the
// canonical Monaco Monarch grammar in
// qu-ui-components/src/components/CodeEditor.tsx (QU_LANGUAGE_CONFIG), plus
// `layer` and `enum`, which are real Qu keywords (see docs/qu-grammar.ebnf's
// LayerClause/SceneItem and engine/crates/qu-syntax/src/lib.rs's enum_stmt)
// that hadn't made it into CodeEditor.tsx's list yet.
(function () {
    if (typeof hljs === 'undefined') {
        return;
    }

    hljs.registerLanguage('qu', function (hljs) {
        var KEYWORDS = {
            keyword:
                'and as assert backend break catch const constant continue data def ' +
                'dimension device each elif else end enum error export false for from ' +
                'function if import in input layer let local model module namespace ' +
                'not on or param read render return select skip step sub table then to ' +
                'train true try type until using warn where while with animate ease ' +
                'frame hold collect method signal spectrum cases otherwise unit repeat ' +
                'loop elsewhere optional pure elemental swap fit use inline project ' +
                'parallel restore every after spawn async await run simd sketch window ' +
                'circuit compose distributed schedule stencil flag mesh scene3d set ' +
                'simulate view watch base class compile finetune implements inherits ' +
                'interface override property tune vectorize node reserve release',
            type:
                'bool int int64 uint uint64 float float64 double complex complex128 ' +
                'string str array vector matrix tensor record list figure logical integer',
            literal: 'true false'
        };

        // `{...}` string interpolation, highlighted distinctly inside strings.
        var SUBST = {
            className: 'subst',
            begin: /\{/,
            end: /\}/,
            keywords: KEYWORDS,
            relevance: 0
        };

        var STRING = {
            className: 'string',
            variants: [
                { begin: '"', end: '"' },
                { begin: "'", end: "'" }
            ],
            contains: [hljs.BACKSLASH_ESCAPE, SUBST]
        };

        var COMMENT = hljs.COMMENT('#', '$');

        var NUMBER = {
            className: 'number',
            variants: [
                { begin: '\\b0[xX][0-9a-fA-F_]+' },
                { begin: '\\b\\d[\\d_]*\\.\\d[\\d_]*([eE][+-]?\\d+)?[ij]?\\b' },
                { begin: '\\b\\d[\\d_]*([eE][+-]?\\d+)?[ij]?\\b' },
                { begin: '\\.\\d[\\d_]*([eE][+-]?\\d+)?[ij]?\\b' }
            ],
            relevance: 0
        };

        var OPERATOR = {
            className: 'operator',
            begin:
                /:=|->|\|>|&&|\|\||==|!=|<=|>=|\+=|-=|\*=|\/=|\.=|\.\*|\.\/|\.\\|\*\*|\?\?|[=<>+\-*/\\^@?!~&|]/,
            relevance: 0
        };

        return {
            name: 'Qu',
            aliases: ['qu'],
            case_insensitive: false,
            keywords: KEYWORDS,
            contains: [COMMENT, STRING, NUMBER, OPERATOR]
        };
    });

    function highlightNode(block) {
        try {
            if (typeof hljs.highlightElement === 'function') {
                hljs.highlightElement(block);
            } else {
                hljs.highlightBlock(block);
            }
        } catch (e) {
            // Leave the block as-is; don't let one bad block break the rest.
            if (window.console && console.warn) {
                console.warn('qu-highlight: failed to highlight block', e);
            }
        }
    }

    var blocks = document.querySelectorAll('code.language-qu');
    for (var i = 0; i < blocks.length; i++) {
        var block = blocks[i];
        if (!block.classList.contains('editable')) {
            highlightNode(block);
        }
    }
})();
