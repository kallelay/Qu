# Retro window-chrome style reference

**Superseded for QuStudio's own chrome, 2026-09-16.** The dots below were
QuStudio's Terminal panel and editor TabBar default from commit `bf72f15`
until Ahmed asked directly to remove them ("there is the 3 points red
yellow green -> remove these") — done in `20829d97`. A later pass then
had to walk back a *third* copy that had been added to the GUI Designer's
canvas on this doc's own recommendation (`1c34b7d4`) before that ruling
was known to it — the title bar without the dots was kept there, since
the feedback was about the dots specifically, not window chrome as a
whole. **Read this file as the CSS/JS asset it still is for `qu run
--report`/`write_report(...)` (unaffected, a printed report is not
QuStudio's own UI), not as a description of QuStudio's current
appearance.**

The traffic-light window chrome (red/yellow/green dots, glossy gradient
header bars) used in the Qu Tour artifact (2026-09-01) and formerly
QuStudio's Terminal panel and editor TabBar default (commit `bf72f15`,
until the removal above). Saved here as a real, reusable asset — not
something to rebuild from memory or redesign each time it's needed (e.g.
for `qu run --report`, see `BACKLOG.md`'s tooling section).

**Now also used by `qu run <file.qu> --report <path.html>` and the
in-script `write_report(path)` builtin** (2026-09-01) — this CSS/JS is
embedded verbatim into the `qu-cli`/`qu-interp` binaries via
`include_str!` (`engine/crates/qu-interp/src/report_template.css`/
`report_template.js`) rather than copy-pasted a second time; see
`qu_interp::report`'s module doc comment for the full design.

## CSS (light/cream code panel + dark terminal panel pairing)

```css
.win {
  border-radius: 8px; overflow: hidden;
  box-shadow: 0 2px 8px rgba(20,40,70,0.18);
  min-width: 0; /* required inside a CSS grid track -- grid items default
    to min-width:auto, which lets an unbreakable <pre> line force the
    whole track (and the page) wider than the viewport. */
}
.win-chrome {
  display: flex; align-items: center; gap: 6px; padding: 7px 10px;
  background: linear-gradient(180deg, #e9edf3, #cdd6e2);
  border-bottom: 1px solid #b7c1cf;
}
.win-dot { width: 10px; height: 10px; border-radius: 50%; box-shadow: inset 0 1px 1px rgba(255,255,255,0.5); }
.win-dot.r { background: radial-gradient(circle at 35% 30%, #ff6b5f, #d33327); }
.win-dot.y { background: radial-gradient(circle at 35% 30%, #ffd25f, #dba020); }
.win-dot.g { background: radial-gradient(circle at 35% 30%, #6de06a, #2ba82b); }
.win-title { font-family: monospace; font-size: 11px; color: #4b5a6b; margin-left: 6px; }

/* Code panel: warm cream, like a classic syntax-highlighted snippet widget */
.code-win .win-chrome { background: linear-gradient(180deg, #f3f0e2, #ddd6bc); border-bottom-color: #c7bd9c; }
.code-win .win-title { color: #6b6248; }
.code-body {
  background: #fdfbf3; color: #3a3527; font-family: monospace; font-size: 12px; line-height: 1.55;
  padding: 12px 14px; margin: 0; overflow-x: auto; max-height: 380px; overflow-y: auto; white-space: pre;
}
.code-body .kw { color: #9c4ba5; font-weight: 700; }  /* keywords */
.code-body .cm { color: #8a9b7a; font-style: italic; } /* comments */
.code-body .str { color: #b5651d; }                    /* strings */
.code-body .num { color: #1f6fa8; }                    /* numbers */

/* Terminal panel: dark, like a real OS X Terminal.app window */
.term-win .win-chrome { background: linear-gradient(180deg, #4a4a4a, #2b2b2b); border-bottom-color: #1a1a1a; }
.term-win .win-title { color: #cfcfcf; }
.term-body {
  background: #1c1e22; color: #d6f5dc; font-family: monospace; font-size: 12px; line-height: 1.55;
  padding: 12px 14px; margin: 0; overflow-x: auto; max-height: 380px; overflow-y: auto;
  white-space: pre-wrap; word-break: break-word;
}
.term-body::before { content: "$ qu run " attr(data-cmd) "\A"; color: #7fb0ff; }
```

## HTML structure per code/output pair

```html
<div class="win code-win">
  <div class="win-chrome"><span class="win-dot r"></span><span class="win-dot y"></span><span class="win-dot g"></span><span class="win-title">demo.qu</span></div>
  <pre class="code-body"><!-- highlightQu(source) output --></pre>
</div>
<div class="win term-win">
  <div class="win-chrome"><span class="win-dot r"></span><span class="win-dot y"></span><span class="win-dot g"></span><span class="win-title">Terminal</span></div>
  <pre class="term-body" data-cmd="demo.qu"><!-- escaped captured stdout --></pre>
</div>
```

## Minimal Qu syntax highlighter (regex-based, "good enough")

```js
function escapeHtml(s) { return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;'); }
const QU_KEYWORDS = /\b(function|end|for|to|step|if|then|else|elif|return|print|while|try|catch|backend|auto|seed|hold|show|plot|legend|title|xlabel|ylabel|subplot|zeros|ones|true|false|none)\b/g;
function highlightQu(code) {
  return code.split('\n').map(line => {
    let commentIdx = -1, inStr = false;
    for (let i = 0; i < line.length; i++) {
      const c = line[i];
      if (c === '"') inStr = !inStr;
      if (c === '#' && !inStr) { commentIdx = i; break; }
    }
    let codePart = commentIdx >= 0 ? line.slice(0, commentIdx) : line;
    let commentPart = commentIdx >= 0 ? line.slice(commentIdx) : '';
    codePart = escapeHtml(codePart)
      .replace(/"([^"]*)"/g, '<span class="str">"$1"</span>')
      .replace(/\b(\d+\.?\d*)\b/g, '<span class="num">$1</span>')
      .replace(QU_KEYWORDS, '<span class="kw">$1</span>');
    commentPart = commentPart ? '<span class="cm">' + escapeHtml(commentPart) + '</span>' : '';
    return codePart + commentPart;
  }).join('\n');
}
```

Real limitation, stated plainly: this highlighter is a simple per-line
regex pass, not a real tokenizer — it can mis-highlight a `#` inside a
string that itself contains an odd number of `"` on the same line, and
the keyword list is hand-picked, not exhaustive. Good enough for a
report/demo page; not a substitute for a real editor grammar (see
`editors/vscode-qu/syntaxes/qu.tmLanguage.json` for that).
