import React, { useEffect, useMemo, useState } from "react";
import { BookOpen, Copy, Search, X } from "lucide-react";

/** One builtin's documentation, exactly `qu docs --json`'s shape (see
 *  `engine/crates/qu-cli/src/main.rs`'s `builtin_docs_json`, itself a
 *  pass-through of `qu-interp`'s compiled-in `BUILTIN_DOCS` table). */
export interface BuiltinDoc {
  name: string;
  signature: string;
  summary: string;
  chapter: string;
}

export interface HelpBrowserProps {
  /** Accepted for call-site symmetry with the other overlays, but no
   *  longer read: this drawer takes its colours from the `--qu-*` tokens
   *  on `:root`, which `ThemeProvider` already keys off `<html
   *  data-theme>`. It used to branch on this prop for exactly two
   *  values (the drawer background and the sticky chapter header) while
   *  every other colour in the file was pinned to a light literal --
   *  i.e. the prop was doing a tenth of the job it looked like it did. */
  theme?: "light" | "dark";
  invoke: <T,>(command: string, args?: Record<string, unknown>) => Promise<T>;
  onClose: () => void;
  /** Jump straight to one name on open (e.g. from a "?" on a squiggle or a
   *  right-click "Look up" action elsewhere in the app). Optional -- the
   *  browser opens to the full list when absent. */
  initialQuery?: string;
}

// This panel is a `position: fixed` child of the app root, not of any
// `.qu-inspector`, so until the token set was promoted to `:root` every
// `var(--qu-*, ...)` in here resolved to its LIGHT literal fallback --
// in dark mode, a #d8d6cd hairline (near white) around every row of a
// near-black drawer. The fallbacks are gone rather than corrected,
// because a fallback that can silently become the whole styling is how
// this went unnoticed.
const border = "1px solid var(--qu-border)";

function chapterLabel(slug: string): string {
  return slug.replace(/-/g, " ").replace(/\b\w/g, (c) => c.toUpperCase());
}

/** A searchable reference over Qu's builtin table -- the visual counterpart
 *  to `help("name")`, which only ever existed at the REPL/script level.
 *  Read-only: this never runs Qu, it only reads the docs table once via
 *  `list_builtin_docs` and filters client-side. */
export const HelpBrowser: React.FC<HelpBrowserProps> = ({ invoke, onClose, initialQuery }) => {
  const [docs, setDocs] = useState<BuiltinDoc[]>([]);
  const [query, setQuery] = useState(initialQuery ?? "");
  const [expanded, setExpanded] = useState<string | null>(initialQuery ?? null);
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let cancelled = false;
    invoke<BuiltinDoc[]>("list_builtin_docs")
      .then((d) => !cancelled && setDocs(d))
      .catch((e) => !cancelled && setError(String(e)))
      .finally(() => !cancelled && setLoading(false));
    return () => {
      cancelled = true;
    };
  }, [invoke]);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    const matches = q
      ? docs.filter((d) => d.name.toLowerCase().includes(q) || d.summary.toLowerCase().includes(q))
      : docs;
    // Exact/prefix name matches first -- typing a real builtin's name
    // should put it at the top, not wherever alphabetical chapter order
    // happens to place it.
    const rank = (d: BuiltinDoc) => (d.name.toLowerCase() === q ? 0 : d.name.toLowerCase().startsWith(q) ? 1 : 2);
    return [...matches].sort((a, b) => (q ? rank(a) - rank(b) || a.name.localeCompare(b.name) : a.name.localeCompare(b.name)));
  }, [docs, query]);

  const grouped = useMemo(() => {
    const byChapter = new Map<string, BuiltinDoc[]>();
    for (const d of filtered) {
      const list = byChapter.get(d.chapter) ?? [];
      list.push(d);
      byChapter.set(d.chapter, list);
    }
    return [...byChapter.entries()].sort((a, b) => a[0].localeCompare(b[0]));
  }, [filtered]);

  return (
    <div
      role="dialog"
      aria-label="Qu builtin reference"
      className="qu-scrim"
      style={{ position: "fixed", inset: 0, zIndex: 60, display: "flex", justifyContent: "flex-end" }}
      onClick={onClose}
      onKeyDown={(e) => e.key === "Escape" && onClose()}
    >
      <div
        onClick={(e) => e.stopPropagation()}
        style={{
          width: "min(460px, 94vw)",
          height: "100%",
          background: "var(--qu-bg)",
          color: "var(--qu-text)",
          display: "flex",
          flexDirection: "column",
          boxShadow: "-16px 0 48px rgb(0 0 0 / 30%)",
        }}
      >
        <div style={{ padding: "14px 16px", borderBottom: border, display: "flex", flexDirection: "column", gap: 10 }}>
          <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
            <BookOpen size={15} />
            <strong style={{ fontSize: 14 }}>Qu reference</strong>
            <span style={{ fontSize: 11.5, color: "var(--qu-muted)" }}>
              {loading ? "loading…" : `${docs.length} builtins`}
            </span>
            <span style={{ marginLeft: "auto" }} />
            <button onClick={onClose} aria-label="Close help browser" style={{ border: "none", background: "transparent", cursor: "pointer" }}>
              <X size={16} />
            </button>
          </div>
          <div style={{ position: "relative" }}>
            <Search size={13} style={{ position: "absolute", left: 9, top: 9, opacity: 0.6 }} />
            <input
              autoFocus
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder="Search by name or description…"
              style={{ width: "100%", padding: "7px 10px 7px 28px", borderRadius: 6, border, fontSize: 13, background: "var(--qu-surface)", color: "inherit" }}
            />
          </div>
        </div>

        <div style={{ flex: 1, overflowY: "auto" }}>
          {error && (
            <p role="alert" style={{ color: "var(--qu-danger)", fontSize: 12.5, padding: 14 }}>
              {error}
            </p>
          )}
          {!loading && !error && filtered.length === 0 && (
            <p style={{ padding: 14, fontSize: 13, color: "var(--qu-muted)" }}>
              No builtin matches &ldquo;{query}&rdquo;.
            </p>
          )}
          {grouped.map(([chapter, entries]) => (
            <div key={chapter}>
              <div
                style={{
                  position: "sticky",
                  top: 0,
                  padding: "6px 16px",
                  fontSize: 11,
                  fontWeight: 600,
                  textTransform: "uppercase",
                  letterSpacing: "0.04em",
                  color: "var(--qu-muted)",
                  background: "var(--qu-surface)",
                  borderBottom: border,
                  borderTop: border,
                }}
              >
                {chapterLabel(chapter)}
              </div>
              {entries.map((d) => {
                const isOpen = expanded === d.name;
                return (
                  <div key={d.name} style={{ borderBottom: border }}>
                    <button
                      onClick={() => setExpanded(isOpen ? null : d.name)}
                      aria-expanded={isOpen}
                      className="qu-help-row"
                      style={{
                        width: "100%",
                        textAlign: "left",
                        padding: "9px 16px",
                        display: "flex",
                        flexDirection: "column",
                        gap: 3,
                        border: "none",
                        cursor: "pointer",
                        color: "inherit",
                      }}
                    >
                      <code style={{ fontSize: 13, fontWeight: 600 }}>{d.name}</code>
                      {!isOpen && (
                        <span style={{ fontSize: 12, color: "var(--qu-muted)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                          {d.summary}
                        </span>
                      )}
                    </button>
                    {isOpen && (
                      <div style={{ padding: "0 16px 12px", display: "flex", flexDirection: "column", gap: 6 }}>
                        <div style={{ display: "flex", alignItems: "flex-start", gap: 6 }}>
                          <code
                            style={{
                              fontSize: 12.5,
                              background: "var(--qu-code-bg)",
                              padding: "4px 8px",
                              borderRadius: 5,
                              flex: 1,
                              overflowX: "auto",
                              whiteSpace: "pre",
                            }}
                          >
                            {d.signature}
                          </code>
                          <button
                            title="Copy signature"
                            onClick={() => void navigator.clipboard?.writeText(d.signature)}
                            style={{ border: "none", background: "transparent", cursor: "pointer", padding: 4 }}
                          >
                            <Copy size={12} />
                          </button>
                        </div>
                        <p style={{ fontSize: 12.5, lineHeight: 1.5, margin: 0, color: "var(--qu-muted)" }}>{d.summary}</p>
                      </div>
                    )}
                  </div>
                );
              })}
            </div>
          ))}
        </div>
      </div>
    </div>
  );
};

export default HelpBrowser;
