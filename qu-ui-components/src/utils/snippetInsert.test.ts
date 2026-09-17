import { describe, expect, it } from 'vitest';
import { planSnippetInsert } from './snippetInsert';

/**
 * The behaviour the old code had, reproduced exactly so the bug is
 * documented rather than described: a zero-width insert at the caret,
 * which is what `editor.executeEdits` with `startColumn === endColumn`
 * does. This is not the implementation under test -- it is the thing the
 * implementation exists to stop.
 */
function oldRawSpliceAtCaret(currentLine: string, column: number, snippet: string): string {
  const before = currentLine.slice(0, column - 1);
  const after = currentLine.slice(column - 1);
  return before + snippet + after;
}

const SNIPPET = 'df = read_csv("data.csv", headers=true)\nprint("{nrow(df)} rows")';

describe('the corruption this exists to prevent', () => {
  it('reproduces the exact damage seen in a user file', () => {
    // Caret at end of a line that already has a statement on it -- the
    // ordinary case after clicking around in code, since getPosition()
    // survives focus moving to the sidebar.
    const line = "title('Cross-Correlation')";
    const damaged = oldRawSpliceAtCaret(line, line.length + 1, SNIPPET);

    expect(damaged.split('\n')[0]).toBe(
      "title('Cross-Correlation')df = read_csv(\"data.csv\", headers=true)"
    );
    // Two statements welded together with no separator: a parse error
    // that reads like the user mistyped, from an action reported as
    // having succeeded.
    expect(damaged).not.toContain("')\n");
  });

  it('does not weld statements together any more', () => {
    const line = "title('Cross-Correlation')";
    const { text, atLineEnd } = planSnippetInsert(line, SNIPPET);
    expect(atLineEnd).toBe(true);
    expect(text.startsWith('\n')).toBe(true);
    // The joined result is what the editor will actually hold.
    expect((line + text).split('\n')[0]).toBe("title('Cross-Correlation')");
    expect((line + text).split('\n')[1]).toBe('df = read_csv("data.csv", headers=true)');
  });
});

describe('planSnippetInsert', () => {
  it('inserts at the caret when the line is empty', () => {
    expect(planSnippetInsert('', SNIPPET)).toEqual({ text: SNIPPET, atLineEnd: false });
  });

  it('treats a whitespace-only line as empty, keeping its indentation', () => {
    // Nothing to collide with, and the indentation the user typed is
    // already in the buffer -- adding another newline would leave a
    // stray blank line above the snippet.
    expect(planSnippetInsert('    ', SNIPPET)).toEqual({ text: SNIPPET, atLineEnd: false });
  });

  it('matches the indentation of the line the caret is on', () => {
    const { text } = planSnippetInsert('    y = 2', 'a = 1\nb = 2');
    expect(text).toBe('\n    a = 1\n    b = 2');
  });

  it('indents with tabs when the line uses tabs', () => {
    const { text } = planSnippetInsert('\t\ty = 2', 'a = 1');
    expect(text).toBe('\n\t\ta = 1');
  });

  it('does not indent blank lines inside a snippet', () => {
    // Trailing whitespace on an otherwise-empty line is noise in a diff
    // and some editors strip it, which would make the file dirty on save.
    const { text } = planSnippetInsert('  x = 1', 'a = 1\n\nb = 2');
    expect(text).toBe('\n  a = 1\n\n  b = 2');
  });

  it('goes to the end of the line, so a caret mid-expression cannot split it', () => {
    // `plot(t, |y)` -- inserting a newline AT the caret would produce two
    // broken lines. Visible, but still wrong.
    const plan = planSnippetInsert('plot(t, y)', 'a = 1');
    expect(plan.atLineEnd).toBe(true);
  });
});
