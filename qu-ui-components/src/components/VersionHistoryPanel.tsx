import React, { useEffect, useState } from "react";
import { DiffEditor } from "@monaco-editor/react";
import { Clock, History, RotateCcw, X } from "lucide-react";

/** One saved snapshot, as reported by the `list_file_versions` Tauri
 *  command (`qu-studio-tauri/src-tauri/src/main.rs`). `timestamp` is
 *  nanoseconds since the Unix epoch, AS A STRING -- also the version's id,
 *  passed back to `read_file_version` verbatim. Kept as a string rather
 *  than `number` on purpose: a nanosecond epoch value needs ~61 bits and a
 *  JS `number` only carries 53 safely, so parsing it here would silently
 *  round to a value that no longer names any real file on disk. Only ever
 *  parsed for DISPLAY (`relativeTime`, which tolerates the same rounding
 *  because a few nanoseconds is far below its own resolution) -- never
 *  round-tripped back to the backend as a number. */
export interface FileVersion {
  timestamp: string;
  size: number;
}

export interface VersionHistoryPanelProps {
  /** The open file's on-disk path -- versions live in a sibling
   *  `.qu-versions/<name>/` folder next to it. */
  path: string;
  /** The buffer as it stands right now, for the diff's "modified" side. */
  currentContent: string;
  theme?: "light" | "dark";
  invoke: <T,>(command: string, args?: Record<string, unknown>) => Promise<T>;
  /** Load a past version's text into the editor. Restoring is a normal
   *  edit the user still has to save -- this never writes to disk itself. */
  onRestore: (content: string) => void;
  onClose: () => void;
}

/** `nanos` is a nanosecond-epoch STRING (see `FileVersion.timestamp`'s own
 *  doc comment for why it isn't a number) -- `Number(...)` here only feeds
 *  a relative-time label, where nanosecond-scale rounding is invisible. */
function relativeTime(nanos: string): string {
  const ms = Number(nanos) / 1e6;
  const diff = Date.now() - ms;
  const s = Math.max(0, Math.floor(diff / 1000));
  if (s < 60) return "just now";
  const m = Math.floor(s / 60);
  if (m < 60) return `${m} min ago`;
  const h = Math.floor(m / 60);
  if (h < 24) return `${h} hr ago`;
  const d = Math.floor(h / 24);
  if (d < 30) return `${d} day${d === 1 ? "" : "s"} ago`;
  return new Date(ms).toLocaleDateString();
}

// Same story as HelpBrowser: fixed-position, outside any
// `.qu-inspector`, so every token fallback in here was the live value
// and the dark theme got light-theme hairlines. Tokens resolve at
// `:root` now (inspector.css), so the fallbacks are removed rather than
// left to shadow the real thing.
const border = "1px solid var(--qu-border)";

/** Version history for one file: a list of past saves down the left, a
 *  read-only diff of the selected one against the current buffer on the
 *  right, and a Restore button. Snapshot-on-save, not continuous history --
 *  see `main.rs`'s `snapshot_before_overwrite`. */
export const VersionHistoryPanel: React.FC<VersionHistoryPanelProps> = ({
  path,
  currentContent,
  theme = "light",
  invoke,
  onRestore,
  onClose,
}) => {
  const [versions, setVersions] = useState<FileVersion[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [selectedContent, setSelectedContent] = useState("");
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError("");
    invoke<FileVersion[]>("list_file_versions", { path })
      .then((v) => {
        if (cancelled) return;
        setVersions(v);
        setSelected(v[0]?.timestamp ?? null);
      })
      .catch((e) => !cancelled && setError(String(e)))
      .finally(() => !cancelled && setLoading(false));
    return () => {
      cancelled = true;
    };
  }, [path, invoke]);

  useEffect(() => {
    if (selected == null) {
      setSelectedContent("");
      return;
    }
    let cancelled = false;
    invoke<string>("read_file_version", { path, timestamp: selected })
      .then((c) => !cancelled && setSelectedContent(c))
      .catch((e) => !cancelled && setError(String(e)));
    return () => {
      cancelled = true;
    };
  }, [selected, path, invoke]);

  return (
    <div
      role="dialog"
      aria-label="Version history"
      className="qu-scrim"
      style={{ position: "fixed", inset: 0, zIndex: 60, display: "flex" }}
      onClick={onClose}
      onKeyDown={(e) => e.key === "Escape" && onClose()}
    >
      <div
        onClick={(e) => e.stopPropagation()}
        style={{
          margin: "auto",
          width: "min(1040px, 94vw)",
          height: "min(700px, 88vh)",
          background: "var(--qu-bg)",
          color: "var(--qu-text)",
          border: border,
          borderRadius: 10,
          display: "flex",
          overflow: "hidden",
          boxShadow: "0 24px 64px rgb(0 0 0 / 40%)",
        }}
      >
        <aside style={{ width: 220, borderRight: border, display: "flex", flexDirection: "column" }}>
          <div style={{ padding: "12px 14px", display: "flex", alignItems: "center", gap: 8, borderBottom: border }}>
            <History size={14} />
            <strong style={{ fontSize: 13 }}>Version history</strong>
          </div>
          <div style={{ flex: 1, overflowY: "auto" }}>
            {loading && (
              <p style={{ padding: 14, fontSize: 12, color: "var(--qu-muted)" }}>Loading…</p>
            )}
            {!loading && versions.length === 0 && (
              <div className="qu-inspector qu-empty" data-theme={theme} style={{ padding: "28px 16px" }}>
                <History size={24} />
                <strong>No earlier versions</strong>
                <p>
                  A snapshot is kept each time you save over this file. The
                  first one lands on your next save.
                </p>
              </div>
            )}
            {versions.map((v) => (
              <button
                key={v.timestamp}
                onClick={() => setSelected(v.timestamp)}
                style={{
                  display: "flex",
                  alignItems: "center",
                  gap: 8,
                  width: "100%",
                  textAlign: "left",
                  padding: "9px 14px",
                  fontSize: 12.5,
                  border: "none",
                  borderLeft: selected === v.timestamp ? "2px solid var(--qu-accent)" : "2px solid transparent",
                  background: selected === v.timestamp ? "var(--qu-accent-soft)" : "transparent",
                  cursor: "pointer",
                  color: "inherit",
                }}
                className="qu-help-row"
              >
                <Clock size={12} style={{ opacity: 0.6, flexShrink: 0 }} />
                <span>{relativeTime(v.timestamp)}</span>
              </button>
            ))}
          </div>
        </aside>
        <main style={{ flex: 1, display: "flex", flexDirection: "column", minWidth: 0 }}>
          <div style={{ padding: "10px 14px", display: "flex", alignItems: "center", gap: 10, borderBottom: border }}>
            <span style={{ fontSize: 12.5, color: "var(--qu-muted)" }}>
              Comparing the selected version (left) against your current buffer (right)
            </span>
            <span style={{ marginLeft: "auto" }} />
            {selected != null && (
              <button
                onClick={() => {
                  onRestore(selectedContent);
                  onClose();
                }}
                style={{
                  display: "flex",
                  alignItems: "center",
                  gap: 6,
                  fontSize: 12.5,
                  padding: "5px 10px",
                  borderRadius: 6,
                  border: "1px solid var(--qu-accent-solid)",
                  background: "var(--qu-accent-solid)",
                  color: "#fff",
                  cursor: "pointer",
                }}
              >
                <RotateCcw size={12} /> Restore this version
              </button>
            )}
            <button
              onClick={onClose}
              aria-label="Close version history"
              style={{ border: "none", background: "transparent", cursor: "pointer" }}
            >
              <X size={16} />
            </button>
          </div>
          <div style={{ flex: 1, minHeight: 0 }}>
            {selected != null ? (
              <DiffEditor
                original={selectedContent}
                modified={currentContent}
                language="qu"
                theme={theme === "dark" ? "qu-dark" : "qu-light"}
                options={{ readOnly: true, renderSideBySide: true, minimap: { enabled: false }, fontSize: 12.5 }}
                height="100%"
              />
            ) : (
              <div style={{ padding: 20, color: "var(--qu-muted)", fontSize: 13 }}>
                {loading ? "" : "Select a version on the left to compare it."}
              </div>
            )}
          </div>
          {error && (
            <p role="alert" style={{ color: "var(--qu-danger)", fontSize: 12, padding: "6px 14px", margin: 0 }}>
              {error}
            </p>
          )}
        </main>
      </div>
    </div>
  );
};

export default VersionHistoryPanel;
