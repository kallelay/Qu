import React, { useState, useEffect, useMemo, useRef } from 'react';
import Editor, { OnMount, BeforeMount, EditorProps, loader } from '@monaco-editor/react';
import * as monacoEditor from 'monaco-editor';
import editorWorker from 'monaco-editor/editor/editor.worker?worker';
import { motion } from 'framer-motion';
import { cn } from '../utils/cn';
import { glassStyle } from '../utils/glassStyle';
import { parseCells, getPrefixSource, QuCell } from '../utils/cells';
import { planSnippetInsert } from '../utils/snippetInsert';
import { columnSelectionRanges } from '../utils/columnSelect';

// ---- Monaco is BUNDLED, not fetched ----
// `@monaco-editor/react`'s default loader pulls Monaco from
// `https://cdn.jsdelivr.net/npm/monaco-editor@<version>/min/vs` at runtime.
// That default was in force here and nobody had noticed, because it is
// invisible on any machine with a working internet connection: Qu Studio is
// a DESKTOP application, so with the CDN default it had no editor at all
// offline -- a blank pane where the code goes. It also meant a remote,
// unpinned script executing inside a webview whose `tauri.conf.json` sets
// `"csp": null`, i.e. with nothing constraining it.
//
// Pointing the loader at the module we import makes the editor part of the
// bundle. It costs bundle size, which is the right trade for an app that is
// installed rather than served.
//
// The worker is required, not optional: Monaco's ESM build throws
// ("You must define a function MonacoEnvironment.getWorkerUrl") the moment
// it wants one. Only the plain editor worker is wired up -- `qu` is a
// Monarch grammar, which tokenizes on the main thread and needs no worker
// of its own, and the other languages in `Language` fall back to basic
// tokenization rather than full language services.
(self as any).MonacoEnvironment = {
  getWorker: () => new editorWorker(),
};
loader.config({ monaco: monacoEditor });

export type Language = 
  | 'qu' 
  | 'matlab' 
  | 'python' 
  | 'javascript' 
  | 'typescript' 
  | 'rust' 
  | 'json' 
  | 'markdown';

/** Payload delivered to `onRunCell` for a single "Run Cell" invocation. */
export interface RunCellPayload {
  /** The cell that was targeted (by CodeLens click, or Shift+Enter at the cursor). */
  cell: QuCell;
  /**
   * The code to actually execute: cells [0..cell.index] concatenated. See
   * `getPrefixSource` in `utils/cells.ts` for why -- in short, there is no
   * persistent Qu process backing the editor, so "cell N sees cell (N-1)'s
   * variables" is achieved by re-running every prior cell, not by real
   * incremental kernel state.
   */
  prefixCode: string;
}

/**
 * One parse-error diagnostic, as reported by a `check_syntax`-style backend
 * command wrapping `qu_syntax::parse`'s `Result<Program, ParseError>` (see
 * `ParseError { msg, span: { line, col } }` in `engine/crates/qu-syntax`).
 * `line`/`column` are 1-based, matching both `Span` and Monaco's own marker
 * coordinates -- no off-by-one translation needed at the call site.
 */
export interface QuDiagnostic {
  message: string;
  line: number;
  column: number;
}

export interface CodeEditorProps extends Omit<EditorProps, 'language' | 'theme' | 'onChange'> {
  language?: Language;
  value?: string;
  onChange?: (value: string) => void;
  onRun?: () => void;
  onSave?: (value: string) => void;
  /**
   * Called to run a single `#%%` cell (via its CodeLens "Run Cell" link, or
   * Shift+Enter with the cursor inside it). Omit this prop to leave cell
   * execution disabled entirely -- the `#%%` separator decorations still
   * render either way, since they're just a harmless visual aid.
   */
  onRunCell?: (payload: RunCellPayload) => void;
  /**
   * F1: context help for the symbol under the cursor. Called with the word
   * at the caret, or `null` when the caret is not on one (whitespace, a
   * bracket, an empty line) so the host can fall back to general help
   * rather than silently doing nothing.
   *
   * The word is resolved HERE rather than in the host app because only the
   * editor knows where the caret is: `onSelectionChange` reports `null` for
   * a collapsed selection, which is the normal case for pressing F1 — you
   * put the cursor in a name, you do not select it first.
   */
  onHelpRequest?: (symbol: string | null) => void;
  /**
   * Enables LLM-backed inline (ghost-text) autocomplete via Monaco's own
   * `registerInlineCompletionsProvider` -- native ghost text, Tab accepts
   * it, no hand-rolled rendering. Called with the plain-text code from the
   * start of the buffer up to the cursor; whatever string it resolves to is
   * shown as the suggestion. Omit this prop to leave inline completions
   * off entirely. This component has no idea what's on the other end (a
   * Tauri `invoke('llm_complete', ...)` call in QuStudio's own `App.tsx`,
   * as of this writing) -- keeping that out of a shared component that
   * also has to work outside Tauri (e.g. Storybook).
   */
  onInlineComplete?: (prefixCode: string) => Promise<string>;
  /**
   * When `false`, inline completions only fire on an explicit trigger
   * (Ctrl+Alt+Space here, or Monaco's own "Trigger Inline Suggestion"
   * command) rather than automatically while the user is typing. Defaults
   * to `true` (automatic), but a local on-device LLM's real latency is
   * seconds, not a cloud autocomplete API's tens of milliseconds -- ghost
   * text popping in long after the user has already kept typing past that
   * point reads as broken, not helpful. Measure actual latency before
   * relying on the default; QuStudio's own `App.tsx` sets this to `false`
   * for exactly that reason -- see its own comment for the measured
   * number.
   */
  autoTriggerInlineComplete?: boolean;
  /**
   * A one-shot request to insert `text` at the current cursor position
   * (falling back to the end of the buffer if the editor never had focus
   * yet), e.g. from a "Snippets" picker in the host app -- distinct from
   * the whole-buffer replace a "Catalog"/template picker does via `value`.
   * Bump `token` on every request (even for the same `text` twice in a
   * row) so the effect below fires each time; the previous `token`'s
   * insert is not re-applied on unrelated re-renders.
   */
  insertRequest?: { text: string; token: number } | null;
  /**
   * A one-shot request to replace an explicit range with `text` -- the
   * "Apply" step of the AI transform/fix review flow (see `AiDiffModal` in
   * this package). Unlike `insertRequest` (always the current cursor
   * position), `range` is captured by the caller at the moment it asked the
   * AI for a suggestion (from `onSelectionChange`'s own `range`), since the
   * user's live selection may have moved/collapsed by the time they click
   * "Apply" on a review panel. Same one-shot `token`-bump contract as
   * `insertRequest`.
   */
  replaceRangeRequest?: {
    text: string;
    range: { startLineNumber: number; startColumn: number; endLineNumber: number; endColumn: number };
    token: number;
  } | null;
  /**
   * Fires whenever the editor's selection changes, with the exact selected
   * text and its range -- `null` when the selection is empty/collapsed to a
   * caret. This is how a host app knows what to send as "the selected code"
   * for the AI transform feature, and what range to hand back to
   * `replaceRangeRequest` once the user accepts a suggestion.
   */
  onSelectionChange?: (
    selection: {
      text: string;
      range: { startLineNumber: number; startColumn: number; endLineNumber: number; endColumn: number };
    } | null
  ) => void;
  /**
   * Inline (squiggle) diagnostics: called with the full buffer text a few
   * hundred ms after the user stops typing (see `DIAGNOSTICS_DEBOUNCE_MS`),
   * expected to resolve with the current list of parse errors -- typically
   * a Tauri `invoke('check_syntax', { code })` call wrapping
   * `qu_syntax::parse` in the host app, but this component has no idea what
   *'s on the other end (same reasoning as `onInlineComplete`: it also has
   * to work outside Tauri, e.g. Storybook). An empty array clears all
   * markers (the "parses cleanly" case). Omit this prop to leave inline
   * diagnostics off entirely -- unlike `onInlineComplete`, this is pure
   * parsing with no LLM involved, so it's expected to resolve in
   * milliseconds, not seconds.
   */
  onCheckSyntax?: (code: string) => Promise<QuDiagnostic[]>;
  showLineNumbers?: boolean;
  showMinimap?: boolean;
  readOnly?: boolean;
  className?: string;
  theme?: 'light' | 'dark' | 'system';
}

// How long an inline-completion request waits, after the provider is asked
// for a suggestion, before it actually calls `onInlineComplete` -- i.e. how
// long the user has to keep typing (or move the cursor, or trigger again)
// before a request is actually sent. Monaco cancels the in-flight request's
// token the moment it's superseded by a newer one, and the check right
// after this delay is what makes that cancellation actually skip the call
// instead of firing it anyway. This is on top of, not instead of, Monaco's
// own per-keystroke re-invocation of the provider -- without this delay,
// EVERY keystroke would kick off a real LLM call, only most of them getting
// cancelled a few ms later; not just wasted CPU, but a burst of requests
// competing for the same single-threaded `Mutex<ModelWeights>` on the Rust
// side (see `llm_bridge.rs`), each queued behind the last.
const INLINE_COMPLETE_DEBOUNCE_MS = 500;

// How long inline diagnostics wait after the last keystroke before actually
// calling `onCheckSyntax`. Much shorter than the LLM-backed
// `INLINE_COMPLETE_DEBOUNCE_MS` above -- this is pure parsing
// (`qu_syntax::parse`), expected to run in milliseconds, not seconds, so
// there's no latency reason to make the user wait as long for a squiggle to
// appear/disappear as for ghost text.
const DIAGNOSTICS_DEBOUNCE_MS = 300;

// The marker "owner" id passed to `monaco.editor.setModelMarkers` for
// diagnostics from `onCheckSyntax`. Monaco keys markers by
// (model, owner), so a distinct, stable owner string here means this
// component's diagnostics can be cleared/replaced independently of markers
// any other feature (a linter, a different provider) might set on the same
// model under its own owner.
const DIAGNOSTICS_MARKER_OWNER = 'qu-diagnostics';

const QU_LANGUAGE_CONFIG = {
  id: 'qu',
  extensions: ['.qu'],
  aliases: ['Qu', 'qu'],
  keywords: [
    'and', 'as', 'assert', 'backend', 'break', 'catch', 'const', 'constant',
    'continue', 'data', 'def', 'dimension', 'device', 'each', 'elif', 'else',
    'end', 'error', 'export', 'false', 'for', 'from', 'function', 'if',
    'import', 'in', 'input', 'let', 'local', 'model', 'module', 'namespace',
    'not', 'on', 'or', 'param', 'read', 'render', 'return', 'select', 'skip',
    'step', 'sub', 'table', 'then', 'to', 'train', 'true', 'try', 'type',
    'until', 'using', 'warn', 'where', 'while', 'with', 'animate', 'ease',
    'frame', 'hold', 'collect', 'method', 'signal', 'spectrum', 'cases',
    'otherwise', 'unit', 'repeat', 'loop', 'elsewhere', 'optional', 'pure',
    'elemental', 'swap', 'fit', 'use', 'inline', 'project', 'parallel',
    'restore', 'every', 'after', 'spawn', 'async', 'await', 'run', 'simd',
    'sketch', 'window', 'circuit', 'compose', 'distributed', 'schedule',
    'stencil', 'flag', 'mesh', 'scene3d', 'set', 'simulate', 'view', 'watch',
    'base', 'class', 'compile', 'finetune', 'implements', 'inherits',
    'interface', 'override', 'property', 'tune', 'vectorize', 'node',
    'reserve', 'release', 'layer', 'enum'
  ],
  typeKeywords: [
    'bool', 'int', 'int64', 'uint', 'uint64', 'float', 'float64', 'double',
    'complex', 'complex128', 'string', 'str', 'array', 'vector', 'matrix',
    'tensor', 'record', 'list', 'figure', 'frame', 'logical', 'integer'
  ],
  operators: [
    '=', '==', '!=', '<', '>', '<=', '>=', '+', '-', '*', '/', '\\',
    '.*', './', '.\\', '^', '**', ':=', '->', '|>', '&&', '||', '!',
    '&', '|', '~', '@', '?', '??', '+=', '-=', '*=', '/=', '.='
  ],
  symbols:  /[=><!~?:&|+\-*\\^@]+/,
  escapes:  /\\(?:[abfnrtv\\"']|x[0-9A-Fa-f]{1,4}|u[0-9A-Fa-f]{4}|U[0-9A-Fa-f]{8})/,
};

/**
 * Notion/Slack-style slash-command snippets: typing `/` at the start of a
 * line (see `SLASH_TRIGGER_RE` in `handleEditorMount`) offers this list via
 * Monaco's own `registerCompletionItemProvider`, each one expanding via
 * `InsertTextRules.InsertAsSnippet` (`${n:placeholder}` tabstops, Tab to
 * hop between them) -- the same provider mechanism that would drive any
 * other autocomplete, just gated on `/` as a trigger character and on
 * cursor context (see below) rather than a separate overlay UI, per the
 * backlog's own preference. A distinct provider from the CodeLens/
 * inline-completion ones registered elsewhere in this file, but the same
 * "register once in `handleEditorMount`, dispose on unmount" shape.
 *
 * Every `insertText` here is real, verified Qu syntax -- lifted from actual
 * working calls in `catalog/*.qu` (e.g. `plot(x, y, label="...",
 * color="...")` in `qu_ar_model_forecast.qu`/`qu_rc_filter.qu`,
 * `histogram(s, bins=50, label="...", color="...")` in
 * `qu_ml_report.qu`, `if ... then / else / end if` in
 * `qu_ar_model_forecast.qu`, `function name(args) ... end function` in
 * `qu_curve_fit.qu`) or the interpreter's own builtin arms in
 * `engine/crates/qu-interp/src/lib.rs` (`"input"` -- despite the keyword's
 * generic name, the only real callable `input(n)` builtin is the ML
 * pipeline-input layer that pairs with `dense_layer`/`sequential`, per that
 * arm's own doc comment -- not a stdin-read, which Qu has no builtin for).
 * None of this is invented syntax.
 */
const SLASH_SNIPPETS: Array<{ trigger: string; insertText: string; detail: string }> = [
  {
    trigger: '/plot',
    insertText: 'plot(${1:x}, ${2:y}, label="${3:}", color="${4:blue}")',
    detail: 'plot(x, y, label=, color=) -- line/marker plot into the current figure',
  },
  {
    trigger: '/scatter',
    insertText: 'scatter(${1:x}, ${2:y}, label="${3:}", color="${4:blue}")',
    detail: 'scatter(x, y, label=, color=) -- marker-only plot',
  },
  {
    trigger: '/hist',
    insertText: 'histogram(${1:samples}, bins=${2:30}, label="${3:}", color="${4:gray}")',
    detail: 'histogram(samples, bins=, label=, color=) -- bin raw samples and plot as bars',
  },
  {
    trigger: '/print',
    insertText: 'print("${1:}")',
    detail: 'print("...") -- write an interpolated string ({expr} / {expr:.3f}) to stdout',
  },
  {
    trigger: '/input',
    insertText: 'input(${1:in_dim})',
    detail: 'input(in_dim) -- ML pipeline input layer; pipe into dense_layer(...) |> sequential(seed=)',
  },
  {
    trigger: '/if',
    insertText: 'if ${1:condition} then\n\t${2:}\nelse\n\t${3:}\nend if',
    detail: 'if <cond> then / else / end if',
  },
  {
    trigger: '/for',
    insertText: 'for ${1:i} = ${2:1} to ${3:N}\n\t${4:}\nend for',
    detail: 'for i = <lo> to <hi> / end for',
  },
  {
    trigger: '/function',
    insertText: 'function ${1:name}(${2:args})\n\t${3:}\n\treturn ${4:}\nend function',
    detail: 'function name(args) ... return ... end function',
  },
  // ---- second batch ----
  // Same standard of evidence as the originals: every body below was run
  // through the engine as one script before being added (`while`/`end
  // while`, `linspace`, `zeros`, `fft`, `figure`/`title`/`xlabel`/
  // `ylabel`/`legend`/`savefig`, `mean`, `std`), not taken from another
  // language's habits or from `help()` output alone. `read_csv` was
  // confirmed a real builtin the same way.
  {
    trigger: '/cell',
    insertText: '#%% ${1:section title}\n${2:}',
    detail: '#%% marker -- starts a runnable cell (Shift+Enter, or its ▶ Run Cell link)',
  },
  {
    trigger: '/while',
    insertText: 'while ${1:condition}\n\t${2:}\nend while',
    detail: 'while <cond> / end while',
  },
  {
    trigger: '/linspace',
    insertText: '${1:x} = linspace(${2:0}, ${3:1}, ${4:100})',
    detail: 'linspace(start, stop, n) -- n evenly spaced values, endpoints included',
  },
  {
    trigger: '/zeros',
    insertText: '${1:z} = zeros(${2:n})',
    detail: 'zeros(n) -- a zero-filled vector; zeros(r, c) for a matrix',
  },
  {
    trigger: '/fft',
    insertText: '${1:Y} = fft(${2:y})',
    detail: 'fft(y) -- discrete Fourier transform of a signal',
  },
  {
    trigger: '/figure',
    insertText: 'figure()\nplot(${1:x}, ${2:y}, label="${3:}", color="${4:blue}")\ntitle("${5:}")\nxlabel("${6:}")\nylabel("${7:}")\nlegend()',
    detail: 'figure() + plot + title/xlabel/ylabel/legend -- a complete labelled figure',
  },
  {
    trigger: '/savefig',
    insertText: 'savefig("${1:figure}.${2:svg}")',
    detail: 'savefig("name.svg") -- write the current figure; call theme("publication") first for print',
  },
  {
    trigger: '/stats',
    insertText: 'print(mean(${1:s}))\nprint(std(${1:s}))',
    detail: 'mean(s) / std(s) -- std is the sample (n-1) form',
  },
  {
    trigger: '/read_csv',
    insertText: '${1:df} = read_csv("${2:data.csv}")',
    detail: 'read_csv("path.csv") -- load a CSV file',
  },
];

// Matches a line's own text from column 1 up to the cursor when it's
// *just* an optional indent followed by `/` and whatever the user has typed
// of a trigger word so far -- e.g. "    /pl" from a `provideCompletionItems`
// call at that cursor. Deliberately anchored to the start of the line (`^`)
// so `/` used as Qu's real division operator mid-expression (`a / b`) never
// matches this and never offers slash-command noise, even though `/` is
// still registered as a Monaco trigger character globally (Monaco calling
// the provider on every `/` is fine -- the provider itself, via this regex,
// is what decides whether that particular `/` means anything).
const SLASH_TRIGGER_RE = /^([ \t]*)\/(\w*)$/;

export const CodeEditor: React.FC<CodeEditorProps> = ({
  language = 'qu',
  value = '',
  onChange,
  onRun,
  onSave,
  onRunCell,
  onHelpRequest,
  onInlineComplete,
  autoTriggerInlineComplete = true,
  insertRequest,
  replaceRangeRequest,
  onSelectionChange,
  onCheckSyntax,
  showLineNumbers = true,
  showMinimap = true,
  readOnly = false,
  className,
  theme = 'system',
  ...props
}) => {
  const [editorInstance, setEditorInstance] = useState<any>(null);
  const [monacoInstance, setMonacoInstance] = useState<any>(null);
  const editorRef = useRef<any>(null);
  const onRunCellRef = useRef(onRunCell);
  const onHelpRequestRef = useRef(onHelpRequest);
  const cellDecorationIdsRef = useRef<string[]>([]);
  const codeLensDisposableRef = useRef<{ dispose: () => void } | null>(null);
  const foldingDisposableRef = useRef<{ dispose: () => void } | null>(null);
  const inlineCompletionDisposableRef = useRef<{ dispose: () => void } | null>(null);
  const slashCommandDisposableRef = useRef<{ dispose: () => void } | null>(null);
  const onInlineCompleteRef = useRef(onInlineComplete);
  const autoTriggerInlineCompleteRef = useRef(autoTriggerInlineComplete);
  const onSelectionChangeRef = useRef(onSelectionChange);
  const selectionDisposableRef = useRef<{ dispose: () => void } | null>(null);
  const middleDragMouseDownRef = useRef<{ dispose: () => void } | null>(null);
  const middleDragMouseMoveRef = useRef<{ dispose: () => void } | null>(null);
  const middleDragCleanupRef = useRef<(() => void) | null>(null);

  // Keep the refs current without re-registering the CodeLens/inline-
  // completion providers (set up once, in handleEditorMount) on every
  // render.
  useEffect(() => {
    onRunCellRef.current = onRunCell;
    onHelpRequestRef.current = onHelpRequest;
  }, [onRunCell, onHelpRequest]);

  useEffect(() => {
    onInlineCompleteRef.current = onInlineComplete;
  }, [onInlineComplete]);

  useEffect(() => {
    autoTriggerInlineCompleteRef.current = autoTriggerInlineComplete;
  }, [autoTriggerInlineComplete]);

  useEffect(() => {
    onSelectionChangeRef.current = onSelectionChange;
  }, [onSelectionChange]);

  // Cells are recomputed from `value` on every change -- parsing is a
  // single linear scan, cheap enough to not bother memo-guarding beyond
  // useMemo's own reference-equality check.
  const cells = useMemo(() => parseCells(value), [value]);

  // Register Qu language definition
  useEffect(() => {
    if (monacoInstance) {
      monacoInstance.languages.register({ id: 'qu' });
      monacoInstance.languages.setMonarchTokensProvider('qu', {
        // Monarch resolves `@keywords`/`@typeKeywords` case-targets against
        // this object's OWN top-level properties, not QU_LANGUAGE_CONFIG
        // (a separate object) -- without these two lines every keyword/type
        // match in the tokenizer below threw "the @ match target ... is not
        // defined" the instant a single character was tokenized.
        keywords: QU_LANGUAGE_CONFIG.keywords,
        typeKeywords: QU_LANGUAGE_CONFIG.typeKeywords,
        tokenizer: {
          root: [
            [/[a-z_$][\w$]*/, {
              cases: {
                '@keywords': 'keyword',
                '@typeKeywords': 'type',
                '@default': 'identifier'
              }
            }],
            [/0[xX][0-9a-fA-F]+/, 'number.hex'],
            [/[+-]?\d+(?:\.\d*)?(?:[eE][+-]?\d+)?(?:i)?/, 'number'],
            [/"/, { token: 'string.quote', bracket: '@open', next: '@string' }],
            [/'/, { token: 'string.single', bracket: '@open', next: '@stringSingle' }],
            [/#.*$/, 'comment'],
            // Escaping each operator for use as a regex literal is already
            // what `.replace` below does -- an earlier version of this line
            // additionally prepended a literal backslash on top of that,
            // which for any operator already containing `\` (Qu's own
            // left-division operator, `\`) produced a regex source ending in
            // an unescaped trailing backslash: a hard parse-time
            // SyntaxError that crashed the whole editor before it could
            // render a single frame.
            ...QU_LANGUAGE_CONFIG.operators.map(op =>
              [new RegExp(op.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')), 'operator']
            )
          ],
          string: [
            [/[^\\"]+/, 'string'],
            [/"/, { token: 'string.quote', bracket: '@close', next: '@pop' }]
          ],
          stringSingle: [
            [/[^\\']+/, 'string'],
            [/'/, { token: 'string.single', bracket: '@close', next: '@pop' }]
          ]
        }
      });

      // Language configuration -- auto-closing brackets/quotes, comment
      // toggling, and auto-indent around Qu's `... end`-style blocks. This
      // was previously missing entirely: the Monarch tokenizer above only
      // covers syntax *highlighting*, and Monaco does not derive any of
      // these editing behaviors from it. Confirmed live before this change:
      // typing `(` did not auto-insert `)`, Ctrl+/ did nothing, and
      // `bracketPairColorization`/`guides.bracketPairs` (both already
      // enabled in this component's `options`) had no bracket pairs to
      // colorize/guide for the `qu` language.
      //
      // Deliberately does NOT list `'` as a quote/auto-closing pair.
      // `qu-lexer::lex_string` (and this file's own Monarch tokenizer, see
      // the `stringSingle` state above) accept `'...'` as an alternate
      // string delimiter lexically, but real Qu code -- every `.qu` file
      // under `catalog/` -- never actually uses `'` that way: it's used
      // exclusively as the postfix transpose operator (`A'`, `Q'`, `V'` in
      // `catalog/qu_linear_algebra.qu` and `catalog/qu_qr_svd.qu`), which
      // the lexer itself resolves contextually via `ends_expr` (a `'`
      // right after something that can end an expression -- an identifier,
      // literal, `)`/`]`/`}` -- is transpose, not a string opener). Monaco's
      // autoClosingPairs has no such context; auto-closing `'` here would
      // turn every `A'` keystroke into `A''` with the cursor stranded
      // between the two quotes, actively fighting the language's real
      // syntax. `"` has no such ambiguity -- it's a pure string delimiter
      // throughout the catalog (281 occurrences, always literal strings)
      // -- so only `"` is configured as an auto-closing/surrounding pair.
      monacoInstance.languages.setLanguageConfiguration('qu', {
        comments: {
          lineComment: '#',
        },
        brackets: [
          ['(', ')'],
          ['[', ']'],
          ['{', '}'],
        ],
        autoClosingPairs: [
          { open: '(', close: ')' },
          { open: '[', close: ']' },
          { open: '{', close: '}' },
          { open: '"', close: '"', notIn: ['string', 'comment'] },
        ],
        surroundingPairs: [
          { open: '(', close: ')' },
          { open: '[', close: ']' },
          { open: '{', close: '}' },
          { open: '"', close: '"' },
        ],
        // Auto-indent around Qu's `<opener> ... end [<tail>]` block shape
        // (spec §16: `if`/`for`/`while`/`function` always reserved keywords;
        // `try`/`unsafe` close with a bare `end`, no tail; `enum`/`pool`/
        // `parallel for`/the `every`/`after`/`at ... do`/`on elapsed(...) do`
        // timer forms/`wait until ... end` are contextual, recognized by
        // `qu-syntax::Parser::statement` by shape rather than by a reserved
        // word -- see that file's own statement-dispatch comments). A line
        // starting a block bumps the next line's indent; a line starting
        // with `end`/`else`/`elseif`/`catch` (the only mid-block dedent
        // keywords -- `try`/`if` bodies) dedents itself back one level.
        indentationRules: {
          increaseIndentPattern: new RegExp(
            '^\\s*(' +
              'if\\b|for\\b|while\\b|function\\b|try\\b|unsafe\\b|' +
              'enum\\s+\\w|pool\\s+\\w+\\s+with\\b|parallel\\s+for\\b|' +
              'wait\\s+until\\b|' +
              '(every|after|at)\\b(?!\\s*(=|:=|\\+=|-=|\\*=|/=|\\.=)).*\\bdo\\b|' +
              'on\\s+elapsed(Once)?\\s*\\(.*\\)\\s*do\\b' +
              ')'
          ),
          decreaseIndentPattern: /^\s*(end\b|else\b|elseif\b|catch\b)/,
        },
      });

    }
  }, [monacoInstance]);

  // Registers the `qu-dark`/`qu-light` custom themes -- this MUST happen in
  // `beforeMount`, not the `[monacoInstance]` effect above (where these two
  // `defineTheme` calls used to live) or `onMount` below. `<Editor
  // theme={resolvedTheme}>` applies its initial theme as part of creating
  // the editor instance itself; `onMount` (and any effect gated on state
  // `onMount` sets) only runs AFTER that instance already exists, i.e.
  // after the first theme application already happened and already failed
  // silently (Monaco falls back to its built-in light `vs` theme for an
  // unrecognized name). That's a real bug that shipped and was reported
  // live (2026-09-04): "the syntax highlighter is always white background
  // even if dark, unless toggled twice" -- the FIRST theme application
  // predates `qu-dark` existing at all, so it silently falls back to
  // light; only a later toggle (by which point the old post-mount effect
  // has had a chance to run) picks up the real theme. `beforeMount` is
  // `@monaco-editor/react`'s own documented hook for exactly this
  // ordering requirement -- it runs before the editor instance (and its
  // initial theme) is created.
  const handleBeforeMount: BeforeMount = (monaco) => {
    monaco.editor.defineTheme('qu-dark', {
      base: 'vs-dark',
      inherit: true,
      rules: [
        { token: 'keyword', foreground: 'c586c0', fontStyle: 'bold' },
        { token: 'type', foreground: '4ec9b0' },
        { token: 'string', foreground: 'ce9178' },
        { token: 'comment', foreground: '6a9955', fontStyle: 'italic' },
        { token: 'number', foreground: 'b5cea8' },
        { token: 'operator', foreground: 'd4d4d4' },
        { token: 'function', foreground: 'dcdcaa' }
      ],
      colors: {
        // Warm near-black, not the old #0e1524 navy. The editor is the
        // largest single surface in the window, and it was the only one
        // in the blue family -- the top bar, sidebar, inspector and every
        // modal around it are warm neutrals, and the light theme has no
        // blue in its chrome at all. The seam ran right down both sides
        // of the editor. #141413 is the value the GUI Designer's own
        // canvas already used for "the dark plane content sits on", so
        // this is adopting an existing decision rather than inventing a
        // fourth grey. Slightly darker than the shell panel (#161615) so
        // the editor still reads as the well, mirroring how the light
        // theme puts a white editor on an off-white shell.
        'editor.background': '#141413',
        'editor.foreground': '#e6e4dd',
        'editor.lineHighlightBackground': '#1c1c1a',
        'editorLineNumber.foreground': '#57564f',
        'editorLineNumber.activeForeground': '#a3a19a',
        'editorGutter.background': '#141413',
        'editorIndentGuide.background1': '#26262300',
        'editorWidget.background': '#1b1b19',
        'editorWidget.border': '#343431',
      }
    });

    monaco.editor.defineTheme('qu-light', {
      base: 'vs',
      inherit: true,
      rules: [
        { token: 'keyword', foreground: '569cd6', fontStyle: 'bold' },
        { token: 'type', foreground: '267f99' },
        { token: 'string', foreground: 'a31515' },
        { token: 'comment', foreground: '008000', fontStyle: 'italic' },
        { token: 'number', foreground: '098658' },
        { token: 'operator', foreground: '000000' },
        { token: 'function', foreground: '795e26' }
      ],
      colors: {
        'editor.background': '#ffffff',
        'editor.foreground': '#1e1e1e',
        'editorLineNumber.foreground': '#237893',
        'editorLineNumber.activeForeground': '#0b216f',
      }
    });
  };

  const handleEditorMount: OnMount = (editor, monaco) => {
    editorRef.current = editor;
    setEditorInstance(editor);
    setMonacoInstance(monaco);

    // Keyboard shortcuts
    editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyS, () => {
      onSave?.(editor.getValue());
    });

    editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyEnter, () => {
      onRun?.();
    });

    editor.addCommand(monaco.KeyMod.Alt | monaco.KeyCode.KeyEnter, () => {
      // Run current line
      const lineNumber = editor.getPosition()?.lineNumber || 1;
      const line = editor.getModel()?.getLineContent(lineNumber) || '';
      console.log('Run line:', line);
    });

    // ---- #%% cell execution ----
    // Re-parses the model's *current* text (not the possibly-stale `value`
    // prop) so this stays correct across edits without re-registering
    // anything. Both the CodeLens command below and the Shift+Enter
    // keybinding funnel through this one function.
    const runCellAtLine = (lineNumber: number) => {
      const handler = onRunCellRef.current;
      const model = editor.getModel();
      if (!handler || !model) return;
      const source = model.getValue();
      const currentCells = parseCells(source);
      const cell =
        currentCells.find((c) => c.startLine === lineNumber) ??
        currentCells.find((c) => lineNumber >= c.startLine && lineNumber <= c.endLine);
      if (!cell) return;
      const prefixCode = getPrefixSource(source, currentCells, cell.index);
      handler({ cell, prefixCode });
    };

    // Jupyter/VS Code convention: run the cell containing the cursor.
    editor.addCommand(monaco.KeyMod.Shift | monaco.KeyCode.Enter, () => {
      const lineNumber = editor.getPosition()?.lineNumber ?? 1;
      runCellAtLine(lineNumber);
    });

    // Ahmed's 2026-09-16 keymap adds Ctrl+F5 for run-cell ALONGSIDE
    // Shift+Enter rather than replacing it -- the map is additive
    // throughout (run-file likewise keeps Ctrl+Enter and gains F5), so
    // both conventions work and nobody's muscle memory breaks.
    //
    // The webview treats Ctrl+F5 as hard-reload, so App.tsx also
    // preventDefaults it at the window level; that guard deliberately does
    // NOT stopPropagation, so this command still receives it.
    editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.F5, () => {
      const lineNumber = editor.getPosition()?.lineNumber ?? 1;
      runCellAtLine(lineNumber);
    });

    // ---- Ctrl+D / Ctrl+Shift+D (Ahmed's ruling, 2026-09-16) ----
    // Ctrl+D is Monaco's own `addSelectionToNextFindMatch` by default --
    // the multicursor workhorse -- so binding duplicate-line to it is a
    // TRADE, not an addition. Put that way, the answer was to move
    // select-next-occurrence to Ctrl+Shift+D rather than let it fall back
    // to Shift+Alt+Down, so it stays one modifier away instead of being
    // relearned.
    //
    // Both delegate to Monaco's own actions via `trigger` rather than
    // reimplementing them: duplicate-line has to cope with multiple
    // cursors, folded regions and column selections, all of which
    // `copyLinesDownAction` already handles.
    editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyD, () => {
      editor.trigger('keymap', 'editor.action.copyLinesDownAction', null);
    });
    editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyMod.Shift | monaco.KeyCode.KeyD, () => {
      editor.trigger('keymap', 'editor.action.addSelectionToNextFindMatch', null);
    });

    // F1: context help for whatever the caret is sitting in. Resolved here
    // because `getWordAtPosition` is the editor's own tokenizer-aware
    // answer -- a host app splitting the line on whitespace would treat
    // `plot(x,` as one word, and would not find `fft` inside `y=fft(x)`.
    // `null` when the caret is not on a word at all, so the host can offer
    // general help rather than silently doing nothing.
    editor.addCommand(monaco.KeyCode.F1, () => {
      const handler = onHelpRequestRef.current;
      if (!handler) return;
      const position = editor.getPosition();
      const model = editor.getModel();
      if (!position || !model) {
        handler(null);
        return;
      }
      handler(model.getWordAtPosition(position)?.word ?? null);
    });


    // A CodeLens "▶ Run Cell" link above each `#%%` marker line. The
    // command id returned by addCommand is unique to *this* editor
    // instance, so multiple mounted CodeEditors each get their own
    // correctly-routed command -- but the CodeLens *provider* itself is
    // registered per-language and is process-wide (Monaco has no
    // per-editor provider scope). QuStudio only ever mounts one Qu
    // CodeEditor at a time today, so this is not a problem in practice;
    // if that ever changes, providers from other mounted instances would
    // also fire for this model (returning identical lenses, since they
    // recompute from the same model text) but their captured commandId
    // would point at the wrong editor. Worth revisiting if/when QuStudio
    // grows split-pane multi-file editing.
    const runCellCommandId = editor.addCommand(0, (_accessor: any, lineNumber: number) => {
      runCellAtLine(lineNumber);
    });

    if (runCellCommandId) {
      const provider = {
        provideCodeLenses: (model: any) => {
          const modelCells = parseCells(model.getValue());
          const lenses = modelCells
            .filter((c) => /^#%%/.test(model.getLineContent(c.startLine) ?? ''))
            .map((c) => ({
              range: {
                startLineNumber: c.startLine,
                startColumn: 1,
                endLineNumber: c.startLine,
                endColumn: 1,
              },
              command: {
                id: runCellCommandId,
                title: c.title ? `▶ Run Cell: ${c.title}` : '▶ Run Cell',
                arguments: [c.startLine],
              },
            }));
          return { lenses, dispose: () => {} };
        },
        resolveCodeLens: (_model: any, codeLens: any) => codeLens,
      };
      const languageId = language === 'qu' ? 'qu' : language;
      codeLensDisposableRef.current = monaco.languages.registerCodeLensProvider(
        languageId,
        provider
      );

      // ---- `#%%` cell folding ----
      // Reuses `parseCells`'s own cell-boundary logic (the same function
      // the CodeLens provider above and the `#%%` separator decoration
      // effect below both already call) rather than re-scanning for `#%%`
      // markers a second, possibly-inconsistent way. A cell folds from its
      // `#%%` marker line down to the line before the next marker (or EOF)
      // -- exactly `QuCell.startLine`/`endLine`. The one-line-file edge
      // case (`startLine === endLine`, an empty or single-line cell) is
      // filtered out: Monaco has nothing meaningful to collapse there, and
      // a zero-height fold region is not useful.
      foldingDisposableRef.current = monaco.languages.registerFoldingRangeProvider(languageId, {
        provideFoldingRanges: (foldModel: any) => {
          const modelCells = parseCells(foldModel.getValue());
          return modelCells
            .filter(
              (c) =>
                /^#%%/.test(foldModel.getLineContent(c.startLine) ?? '') &&
                c.endLine > c.startLine
            )
            .map((c) => ({
              start: c.startLine,
              end: c.endLine,
              kind: monaco.languages.FoldingRangeKind.Region,
            }));
        },
      });
    }

    // ---- LLM-backed inline (ghost-text) autocomplete ----
    // Registered unconditionally (cheap) so toggling `onInlineComplete` on
    // a later render doesn't need a re-mount -- the provider itself checks
    // `onInlineCompleteRef.current` on every call and returns no items when
    // it's unset.
    const inlineLanguageId = language === 'qu' ? 'qu' : language;
    inlineCompletionDisposableRef.current = monaco.languages.registerInlineCompletionsProvider(
      inlineLanguageId,
      {
        provideInlineCompletions: async (model: any, position: any, context: any, token: any) => {
          const handler = onInlineCompleteRef.current;
          if (!handler) return { items: [] };

          const isAutomatic =
            context.triggerKind === monaco.languages.InlineCompletionTriggerKind.Automatic;
          if (isAutomatic && !autoTriggerInlineCompleteRef.current) {
            return { items: [] };
          }

          // Debounce: see INLINE_COMPLETE_DEBOUNCE_MS's own comment. Monaco
          // cancels `token` the instant this request is superseded by a
          // newer keystroke/trigger, which the check right after this wait
          // picks up -- so a burst of keystrokes results in exactly one
          // real `onInlineComplete` call (for whichever request was still
          // current when the debounce elapsed), not one per keystroke.
          await new Promise((resolve) => setTimeout(resolve, INLINE_COMPLETE_DEBOUNCE_MS));
          if (token.isCancellationRequested) return { items: [] };

          const prefixCode = model.getValueInRange({
            startLineNumber: 1,
            startColumn: 1,
            endLineNumber: position.lineNumber,
            endColumn: position.column,
          });
          if (!prefixCode.trim()) return { items: [] };

          let completion: string;
          try {
            completion = await handler(prefixCode);
          } catch {
            // A failed completion (model not loaded, `llm` feature off,
            // generation error) is silent ghost-text-wise -- surfacing it
            // as an editor error for every debounced keystroke would be
            // far noisier than just showing no suggestion this time.
            return { items: [] };
          }
          if (token.isCancellationRequested || !completion || !completion.trim()) {
            return { items: [] };
          }

          return {
            items: [
              {
                insertText: completion,
                range: new monaco.Range(
                  position.lineNumber,
                  position.column,
                  position.lineNumber,
                  position.column
                ),
              },
            ],
          };
        },
        // Named `disposeInlineCompletions` as of this Monaco version
        // (0.56 -- `freeInlineCompletions` was the name in older releases
        // and this component targeted that name at first, which throws
        // `this.provider.disposeInlineCompletions is not a function` at
        // runtime the moment Monaco tries to garbage-collect a completions
        // list, confirmed by actually triggering a completion and reading
        // the console). No cleanup needed either way -- the suggestion
        // string returned by `provideInlineCompletions` isn't holding any
        // resource that needs releasing.
        disposeInlineCompletions: () => {},
      }
    );


    // ---- Middle-mouse-drag column selection ----
    // Monaco has column selection (Shift+Alt+drag, Ctrl+Shift+Alt+arrows)
    // but NO middle-button binding of any kind -- this is not an option
    // that was switched off, there is nothing to switch on, so the gesture
    // has to be built.
    //
    // Implemented by setting one single-line selection per row rather than
    // by reaching into Monaco's internal column-select commands: a column
    // selection IS n single-line selections sharing a column span, and
    // `setSelections` is public API that will not move under us.
    //
    // Columns are clamped per line to that line's own max column, because
    // a rectangle dragged across ragged text extends past the end of the
    // short lines. Monaco's native column select would put the caret in
    // virtual space there; clamping instead means a shorter line
    // contributes a shorter (possibly empty) selection, which is what the
    // same gesture does in every editor that has it.
    let columnAnchor: { lineNumber: number; column: number } | null = null;

    const applyColumnSelection = (target: { lineNumber: number; column: number }) => {
      const model = editor.getModel();
      if (!columnAnchor || !model) return;
      // Geometry lives in `utils/columnSelect.ts` and is unit tested there.
      // It has to be, rather than being a loop right here: Monaco's
      // `onMouseMove` cannot be driven by synthetic events, so a live test
      // of this gesture can press the button but not move it -- the
      // rectangle logic is precisely the part no browser-level test can
      // reach, and the part with the edge cases (lines too short for the
      // rectangle, upward drags, right-to-left drags).
      const ranges = columnSelectionRanges(columnAnchor, target, (lineNumber) =>
        model.getLineMaxColumn(lineNumber)
      );
      if (ranges.length > 0) {
        editor.setSelections(
          ranges.map((r) => new monaco.Selection(r.lineNumber, r.startColumn, r.lineNumber, r.endColumn))
        );
      }
    };

    middleDragMouseDownRef.current = editor.onMouseDown((e: any) => {
      if (!e.event?.middleButton || !e.target?.position) return;
      // Without this the host browser/webview takes the middle button for
      // autoscroll (Windows) or a primary-selection paste (X11), either of
      // which would fight the drag and, in the paste case, silently modify
      // the buffer.
      e.event.preventDefault();
      e.event.stopPropagation();
      columnAnchor = { ...e.target.position };
      applyColumnSelection(e.target.position);
    });

    middleDragMouseMoveRef.current = editor.onMouseMove((e: any) => {
      // `onMouseMove` fires whether or not a button is held, so the anchor
      // -- set on middle-down, cleared on any mouse-up -- is what says a
      // drag is actually in progress.
      if (!columnAnchor || !e.target?.position) return;
      applyColumnSelection(e.target.position);
    });

    // On `window`, not the editor: releasing outside the editor's own DOM
    // node (over the sidebar, the terminal, past the window edge) still
    // ends the drag, rather than leaving the anchor set so the next
    // unrelated mouse move keeps re-selecting.
    const endMiddleDrag = () => {
      columnAnchor = null;
    };
    window.addEventListener('mouseup', endMiddleDrag);
    middleDragCleanupRef.current = () => window.removeEventListener('mouseup', endMiddleDrag);

    // ---- Slash-command snippet palette ----
    // See `SLASH_SNIPPETS`/`SLASH_TRIGGER_RE`'s own doc comments above for
    // the exact trigger shape and where each snippet's syntax was verified.
    // Qu-only (matches whatever this editor's Monaco language id actually
    // is via `inlineLanguageId` above, but the snippets themselves are Qu
    // builtins, so there's nothing useful to offer for `matlab`/`python`/
    // etc. buffers).
    if (inlineLanguageId === 'qu') {
      slashCommandDisposableRef.current = monaco.languages.registerCompletionItemProvider('qu', {
        triggerCharacters: ['/'],
        provideCompletionItems: (model: any, position: any) => {
          const textUntilCursor = model.getValueInRange({
            startLineNumber: position.lineNumber,
            startColumn: 1,
            endLineNumber: position.lineNumber,
            endColumn: position.column,
          });
          const match = textUntilCursor.match(SLASH_TRIGGER_RE);
          // Not "start of line, optional indent, then `/word-so-far`" --
          // most commonly Qu's own division operator (`a / b`) triggering
          // this provider on its own `/`, per Monaco's `triggerCharacters`
          // contract of calling the provider on every typed trigger char,
          // not just ones the provider cares about. No items in that case,
          // same as a completion provider with nothing relevant to say.
          if (!match) return { suggestions: [] };
          const slashColumn = match[1].length + 1; // 1-based column of the '/' itself
          const range = new monaco.Range(position.lineNumber, slashColumn, position.lineNumber, position.column);
          return {
            suggestions: SLASH_SNIPPETS.map((snippet) => ({
              label: snippet.trigger,
              kind: monaco.languages.CompletionItemKind.Snippet,
              insertText: snippet.insertText,
              insertTextRules: monaco.languages.CompletionItemInsertTextRule.InsertAsSnippet,
              detail: snippet.detail,
              // Explicit range (from the '/' through the cursor) so
              // accepting the item replaces the whole typed "/xxx" --
              // Qu has no '/'-prefixed syntax of its own, so none of it
              // should survive into the buffer.
              range,
              // Matched against the "/xxx" text actually typed within
              // `range`, independent of Monaco's own default word-boundary
              // detection (which doesn't treat '/' as part of a word) --
              // without this, filtering/ranking against the typed prefix
              // could behave oddly since the query includes a non-word
              // leading character.
              filterText: snippet.trigger,
              sortText: snippet.trigger,
            })),
          };
        },
      });
    }

    // Explicit trigger, independent of whatever Monaco's own default
    // keybinding for "Trigger Inline Suggestion" is (and independent of
    // `autoTriggerInlineComplete`, so it always works even with automatic
    // triggering turned off) -- the brief's "explicit-trigger keybinding"
    // fallback for when automatic-as-you-type latency is too disruptive.
    editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyMod.Alt | monaco.KeyCode.Space, () => {
      editor.getAction('editor.action.inlineSuggest.trigger')?.run();
    });

    // ---- Selection tracking, for the AI "transform selected code" feature ----
    // Reports the exact selected text + range on every real selection
    // change, straight from the model (`getValueInRange`), not some derived
    // approximation -- a caret with no selection (an empty/collapsed range,
    // `selection.isEmpty()`) reports `null` rather than an empty string, so
    // callers can tell "nothing selected" apart from "selected empty text"
    // (which can't actually happen, but `null` is the unambiguous signal
    // either way).
    selectionDisposableRef.current = editor.onDidChangeCursorSelection((e: any) => {
      const handler = onSelectionChangeRef.current;
      if (!handler) return;
      const selection = e.selection;
      const model = editor.getModel();
      if (!model || !selection || selection.isEmpty()) {
        handler(null);
        return;
      }
      const text = model.getValueInRange(selection);
      if (!text) {
        handler(null);
        return;
      }
      handler({
        text,
        range: {
          startLineNumber: selection.startLineNumber,
          startColumn: selection.startColumn,
          endLineNumber: selection.endLineNumber,
          endColumn: selection.endColumn,
        },
      });
    });
  };

  // Dispose the CodeLens/inline-completion provider registrations on
  // unmount so remounts (hot reload, switching files/tabs at a level that
  // remounts this component) don't stack duplicate providers.
  useEffect(() => {
    return () => {
      codeLensDisposableRef.current?.dispose();
      foldingDisposableRef.current?.dispose();
      inlineCompletionDisposableRef.current?.dispose();
      slashCommandDisposableRef.current?.dispose();
      selectionDisposableRef.current?.dispose();
      middleDragMouseDownRef.current?.dispose();
      middleDragMouseMoveRef.current?.dispose();
      middleDragCleanupRef.current?.();
    };
  }, []);

  // Insert-at-cursor for a "Snippets" picker in the host app -- distinct
  // from `value` (a whole-buffer replace, what the "Catalog"/template
  // pickers use). Tracks the last-applied `token` so this only fires once
  // per request, not on every unrelated re-render while `insertRequest`
  // stays referentially the same object from the caller's last render.
  const lastInsertTokenRef = useRef<number | null>(null);
  useEffect(() => {
    if (!insertRequest || !editorInstance) return;
    if (lastInsertTokenRef.current === insertRequest.token) return;
    lastInsertTokenRef.current = insertRequest.token;

    const editor = editorInstance;
    editor.focus();
    const model = editor.getModel();
    const position = editor.getPosition() ?? model?.getFullModelRange().getEndPosition();
    if (!position || !model) return;

    // A snippet is a statement, so it must not be welded onto whatever
    // the caret happened to be sitting after. This used to be a raw
    // zero-width splice at the caret, which produced things like
    // `title('Cross-Correlation')df = read_csv(...)` in a real user file
    // -- silently, reported as success, surfacing later as a parse error
    // that read like the user's own typo. `getPosition()` keeps the last
    // caret position while focus is in the sidebar, so clicking a snippet
    // from the picker hit this without the caret being anywhere the user
    // was looking. See `planSnippetInsert` for the rule and the trade.
    const plan = planSnippetInsert(model.getLineContent(position.lineNumber), insertRequest.text);
    // Going to the line's end (rather than inserting a newline AT the
    // caret) is what stops a caret parked mid-expression from splitting
    // it: `plot(t, |y)` would otherwise become two broken lines.
    const column = plan.atLineEnd ? model.getLineMaxColumn(position.lineNumber) : position.column;
    const range = {
      startLineNumber: position.lineNumber,
      startColumn: column,
      endLineNumber: position.lineNumber,
      endColumn: column,
    };
    editor.executeEdits('insert-snippet', [{ range, text: plan.text, forceMoveMarkers: true }]);
    editor.focus();
  }, [insertRequest, editorInstance]);

  // "Apply" step of the AI transform/fix review flow -- replaces an
  // EXPLICIT range (captured by the caller back when it asked for a
  // suggestion, via `onSelectionChange`) rather than the current cursor
  // position/selection, since the user may have clicked around the review
  // panel (losing focus, collapsing the selection) before deciding to
  // apply it. Same one-shot `token`-bump contract as `insertRequest`.
  const lastReplaceTokenRef = useRef<number | null>(null);
  useEffect(() => {
    if (!replaceRangeRequest || !editorInstance) return;
    if (lastReplaceTokenRef.current === replaceRangeRequest.token) return;
    lastReplaceTokenRef.current = replaceRangeRequest.token;

    const editor = editorInstance;
    editor.focus();
    editor.executeEdits('ai-replace-range', [
      { range: replaceRangeRequest.range, text: replaceRangeRequest.text, forceMoveMarkers: true },
    ]);
    editor.focus();
  }, [replaceRangeRequest, editorInstance]);

  // Subtle horizontal separator above each `#%%` marker line. Decorations
  // (not a language token) so they show regardless of what Monarch grammar
  // is active, and update on every edit without touching the tokenizer.
  useEffect(() => {
    if (!editorInstance || !monacoInstance) return;
    const model = editorInstance.getModel();
    if (!model) return;
    const decorations = cells
      .filter((c) => /^#%%/.test(model.getLineContent(c.startLine) ?? ''))
      .map((c) => ({
        range: new monacoInstance.Range(c.startLine, 1, c.startLine, 1),
        options: {
          isWholeLine: true,
          className: 'qu-cell-separator-line',
        },
      }));
    cellDecorationIdsRef.current = editorInstance.deltaDecorations(
      cellDecorationIdsRef.current,
      decorations
    );
  }, [cells, editorInstance, monacoInstance]);

  // ---- Inline diagnostics (parse errors as you type) ----
  // Debounced on `value` (see `DIAGNOSTICS_DEBOUNCE_MS`'s own comment for
  // why this can be much shorter than the LLM-backed inline-completion
  // debounce), then rendered as real Monaco squiggles via
  // `setModelMarkers` -- hover shows `message` verbatim from the parser's
  // own `ParseError`. A monotonic request counter (rather than relying on
  // the debounce's own timer id) discards a stale response that resolves
  // after a newer edit has already superseded it -- e.g. a slow first call
  // racing a fast second one.
  const diagnosticsRequestIdRef = useRef(0);
  useEffect(() => {
    if (!onCheckSyntax || !editorInstance || !monacoInstance) return;

    const requestId = ++diagnosticsRequestIdRef.current;
    const timer = setTimeout(() => {
      onCheckSyntax(value)
        .then((diagnostics) => {
          // Superseded by a newer edit while this request was in flight.
          if (requestId !== diagnosticsRequestIdRef.current) return;
          const model = editorInstance.getModel();
          if (!model) return;
          const markers = diagnostics.map((d) => ({
            severity: monacoInstance.MarkerSeverity.Error,
            message: d.message,
            startLineNumber: d.line,
            startColumn: d.column,
            endLineNumber: d.line,
            endColumn: d.column + 1,
          }));
          monacoInstance.editor.setModelMarkers(model, DIAGNOSTICS_MARKER_OWNER, markers);
        })
        .catch(() => {
          // A failed check (backend not wired, transient IPC error) should
          // not spam the editor with a fake error banner -- just leave
          // whatever markers were already showing from the last successful
          // check.
        });
    }, DIAGNOSTICS_DEBOUNCE_MS);

    return () => clearTimeout(timer);
  }, [value, onCheckSyntax, editorInstance, monacoInstance]);

  // Clear this component's own markers on unmount / when `onCheckSyntax` is
  // removed, so a remount or a host toggling the feature off doesn't leave
  // stale squiggles behind on a model that's about to be reused.
  useEffect(() => {
    if (onCheckSyntax || !editorInstance || !monacoInstance) return;
    const model = editorInstance.getModel();
    if (model) {
      monacoInstance.editor.setModelMarkers(model, DIAGNOSTICS_MARKER_OWNER, []);
    }
  }, [onCheckSyntax, editorInstance, monacoInstance]);

  const resolvedTheme = theme === 'system' 
    ? window.matchMedia('(prefers-color-scheme: dark)').matches ? 'qu-dark' : 'qu-light'
    : `qu-${theme}`;

  return (
    <div className={cn("relative h-full w-full overflow-hidden", className)}>
      {/* `#%%` cell boundary marker -- a subtle top border drawn above each
          marker line via a Monaco line decoration (see the effect above). */}
      <style>{`.qu-cell-separator-line { border-top: 1px dashed rgba(137, 135, 129, 0.4); }`}</style>
      <motion.div
        initial={{ opacity: 0 }}
        animate={{ opacity: 1 }}
        transition={{ duration: 0.2 }}
        className="h-full w-full"
      >
        <Editor
          height="100%"
          language={language === 'qu' ? 'qu' : language}
          value={value}
          onChange={(val) => onChange?.(val || '')}
          beforeMount={handleBeforeMount}
          onMount={handleEditorMount}
          theme={resolvedTheme}
          options={{
            readOnly,
            minimap: { enabled: showMinimap },
            lineNumbers: showLineNumbers ? 'on' : 'off',
            fontSize: 14,
            fontFamily: '"Cascadia Mono", "Fira Code", Consolas, monospace',
            fontLigatures: true,
            wordWrap: 'on',
            automaticLayout: true,
            scrollBeyondLastLine: false,
            padding: { top: 16, bottom: 16 },
            cursorBlinking: 'smooth',
            cursorSmoothCaretAnimation: 'on',
            smoothScrolling: true,
            bracketPairColorization: { enabled: true },
            guides: {
              bracketPairs: true,
              indentation: true,
            },
            renderWhitespace: 'selection',
            // Monaco does not enable this by default: without it, Ctrl+wheel
            // just scrolls (same as a plain wheel) instead of zooming font
            // size, unlike every other Monaco-based editor (VS Code, etc.)
            // where it's the expected behavior.
            mouseWheelZoom: true,
            // Native Monaco ghost-text rendering + Tab-to-accept for
            // `onInlineComplete`'s suggestions -- no custom rendering code
            // needed, this is Monaco's own built-in inline-completions UX
            // (the same mechanism VS Code's Copilot integration itself
            // uses). `enabled: true` is Monaco's own default already, spelt
            // out here so it's not accidentally relying on that default.
            inlineSuggest: { enabled: true, mode: 'subwordSmart' },
            ...props.options,
          }}
        />
      </motion.div>
    </div>
  );
};

export default CodeEditor;
