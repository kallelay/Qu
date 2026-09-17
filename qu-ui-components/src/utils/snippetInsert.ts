/**
 * Where a snippet's text should actually go when the user clicks one in a
 * Snippets picker.
 *
 * WHY THIS IS ITS OWN MODULE AND NOT THREE LINES IN THE EDITOR
 * It used to be three lines in the editor, and it silently corrupted
 * users' files. `CodeEditor` inserted the snippet at a zero-width range at
 * the caret with no newline handling, and `stripSnippetHeader` in the host
 * app removes leading newlines from the body (correctly -- that is its
 * job), so the text always began with code. Click a snippet while the
 * caret sat at the end of a line with something on it and you got:
 *
 *     title('Cross-Correlation')df = read_csv("data.csv", headers=true)
 *
 * two statements welded together. The terminal said "Snippet: inserted
 * ..." and the damage surfaced later as a PARSE ERROR pointing at the
 * user's own file -- silent at the moment it happened, reported as
 * success, and finally presented as though the user had mistyped.
 *
 * `editor.getPosition()` returns the LAST caret position even while focus
 * is in the sidebar, so this needed no unusual behaviour from the user:
 * click anywhere in code, click a snippet, and the caret was somewhere
 * they were no longer looking.
 *
 * The rule: a snippet is a statement, so it starts on a line of its own
 * unless the caret's line is genuinely empty. Worst case the user gets a
 * blank line they did not want -- visible, and one undo away. That is the
 * right trade against a corruption they cannot see.
 */

export interface SnippetInsertPlan {
  /** Exact text to hand to the editor's edit operation. */
  text: string;
  /**
   * When true, insert at the END of the caret's line rather than at the
   * caret itself. This is what stops a snippet from splitting an
   * expression the caret happens to be sitting inside -- inserting a
   * newline at the caret would turn `plot(t, |y)` into two broken lines,
   * which is visible but still wrong.
   */
  atLineEnd: boolean;
}

/** The leading whitespace of a line, i.e. the indentation to match. */
function indentOf(line: string): string {
  return /^[ \t]*/.exec(line)?.[0] ?? '';
}

/**
 * Decide how to insert `snippet` given the caret's line and column.
 *
 * @param currentLine full text of the line the caret is on
 * @param snippet     snippet body, header already stripped by the host
 */
export function planSnippetInsert(currentLine: string, snippet: string): SnippetInsertPlan {
  // A blank or whitespace-only line is already a home for a statement:
  // insert where the caret is and let the existing indentation stand.
  if (currentLine.trim() === '') {
    return { text: snippet, atLineEnd: false };
  }

  // Otherwise the line has code on it. Go to a new line, and carry that
  // line's indentation onto every line of the snippet so a snippet
  // dropped inside a loop or an `if` lands at the right depth instead of
  // flush against the margin.
  const indent = indentOf(currentLine);
  const body = snippet
    .split('\n')
    .map((line, i) => (i === 0 || line.trim() === '' ? line : indent + line))
    .join('\n');
  return { text: `\n${indent}${body}`, atLineEnd: true };
}
