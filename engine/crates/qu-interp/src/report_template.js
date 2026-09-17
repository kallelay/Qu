// Minimal Qu syntax highlighter, copied verbatim from
// docs/design/retro-window-chrome.md (see that file for its own stated
// limitations -- a per-line regex pass, not a real tokenizer).
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

// Report-specific bootstrap (not part of the design doc): each code panel
// is rendered by the Rust side as an EMPTY <pre class="code-body"> whose
// raw (unescaped) source text lives in its own `data-qu-source` attribute
// -- attribute values are HTML-entity-decoded by the browser before JS
// ever sees them, so this recovers the exact original source text and
// runs it through `highlightQu` above, unmodified, on load.
document.addEventListener('DOMContentLoaded', function () {
  document.querySelectorAll('.code-body[data-qu-source]').forEach(function (el) {
    el.innerHTML = highlightQu(el.getAttribute('data-qu-source'));
  });
});
