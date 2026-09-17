import React, { useEffect, useMemo, useState } from "react";
import {
  ArrowDownAZ,
  ArrowUpAZ,
  Check,
  ChevronDown,
  ChevronLeft,
  ChevronRight,
  Copy,
  Database,
  Download,
  Search,
  X,
} from "lucide-react";
import { cn } from "../utils/cn";
import {
  downloadBlob,
  formatNumber,
  matrixValue,
  numericSummary,
  sampleSeries,
  variableCsv,
} from "../utils/variableData";
import type { NumericVariable } from "../utils/variableData";
import { PlotViewer } from "./PlotViewer";
import "./inspector.css";

export interface Variable {
  name: string;
  type: string;
  value: string;
  size?: string;
}
export interface VariableExplorerProps {
  variables?: Variable[];
  numericData?: NumericVariable[];
  theme?: "light" | "dark";
  isRunning?: boolean;
  className?: string;
}

const PAGE_SIZE = 50;
const ROWS = 10;
const COLS = 5;

function VariableDetail({
  variable,
  numeric,
  theme,
}: {
  variable: Variable;
  numeric?: NumericVariable;
  theme: "light" | "dark";
}) {
  const [view, setView] = useState<"summary" | "data" | "plot">("summary");
  const [rowPage, setRowPage] = useState(0);
  const [colPage, setColPage] = useState(0);
  const [copyState, setCopyState] = useState("");
  const summary = useMemo(
    () => (numeric ? numericSummary(numeric.data) : null),
    [numeric],
  );
  const samples = useMemo(
    () => (numeric ? sampleSeries(numeric.data) : []),
    [numeric],
  );
  const [rows = 0, columns = 1] = numeric?.shape ?? [];
  const rowStart =
    Math.min(rowPage, Math.max(0, Math.ceil(rows / ROWS) - 1)) * ROWS;
  const colStart =
    Math.min(colPage, Math.max(0, Math.ceil(columns / COLS) - 1)) * COLS;
  const isMatrix = numeric?.type === "matrix" && columns > 1;
  useEffect(() => {
    if (!copyState) return;
    const timer = window.setTimeout(() => setCopyState(""), 2000);
    return () => window.clearTimeout(timer);
  }, [copyState]);

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(
        numeric ? variableCsv(numeric) : variable.value,
      );
      setCopyState("Copied");
    } catch {
      setCopyState("Copy unavailable");
    }
  };

  return (
    <section
      className="qu-variable-detail"
      aria-label={`${variable.name} details`}
    >
      <div className="qu-toolbar">
        <strong className="qu-mono qu-truncate" title={variable.name}>
          {variable.name}
        </strong>
        <span className="qu-badge">
          {numeric
            ? `${rows.toLocaleString()} × ${columns.toLocaleString()}`
            : variable.type}
        </span>
        <span className="qu-spacer" />
        <button
          className="qu-icon-button"
          onClick={copy}
          title="Copy values"
          aria-label="Copy values"
        >
          {copyState === "Copied" ? <Check size={14} /> : <Copy size={14} />}
        </button>
        {numeric && (
          <button
            className="qu-icon-button"
            title="Export all values as CSV"
            aria-label="Export all values as CSV"
            onClick={() =>
              downloadBlob(
                new Blob([variableCsv(numeric)], {
                  type: "text/csv;charset=utf-8",
                }),
                `${variable.name}.csv`,
              )
            }
          >
            <Download size={14} />
          </button>
        )}
      </div>
      {copyState && (
        <div className="qu-note" role="status">
          {copyState}
        </div>
      )}
      {numeric && (
        <div className="qu-segments" aria-label="Variable detail view">
          {(["summary", "data", "plot"] as const).map((item) => (
            <button
              key={item}
              aria-pressed={view === item}
              onClick={() => setView(item)}
            >
              {item === "summary"
                ? "Overview"
                : item === "data"
                  ? "Data"
                  : "Plot"}
            </button>
          ))}
        </div>
      )}
      {(!numeric || view === "summary") && (
        <>
          {summary && (
            <>
              <div className="qu-stats">
                {(
                  [
                    ["Min", summary.min],
                    ["Mean", summary.mean],
                    ["Max", summary.max],
                  ] as const
                ).map(([label, value]) => (
                  <div key={label}>
                    <span>{label}</span>
                    <strong
                      title={value == null ? "No finite values" : String(value)}
                    >
                      {formatNumber(value)}
                    </strong>
                  </div>
                ))}
              </div>
              {numeric && numeric.data.length > 1 && !isMatrix && (
                <div className="qu-sparkline">
                  <svg
                    viewBox="0 0 320 64"
                    role="img"
                    aria-label={`Sampled profile of ${variable.name}`}
                  >
                    {[16, 32, 48].map((y) => (
                      <line
                        key={y}
                        x1="0"
                        x2="320"
                        y1={y}
                        y2={y}
                        className="qu-spark-grid"
                      />
                    ))}
                    <path
                      fill="none"
                      stroke="currentColor"
                      strokeWidth="1.8"
                      strokeLinejoin="round"
                      d={(() => {
                        const scale =
                          Math.max(
                            Math.abs(summary.min ?? 0),
                            Math.abs(summary.max ?? 0),
                          ) || 1;
                        const min = (summary.min ?? 0) / scale;
                        const range = (summary.max ?? 0) / scale - min;
                        let penDown = false;
                        return samples
                          .map((point) => {
                            if (point.y == null) {
                              penDown = false;
                              return "";
                            }
                            const command = penDown ? "L" : "M";
                            penDown = true;
                            return `${command}${2 + ((point.x - 1) / Math.max(1, numeric.data.length - 1)) * 316},${range ? 58 - ((point.y / scale - min) / range) * 52 : 32}`;
                          })
                          .join(" ");
                      })()}
                    />
                  </svg>
                  <span>
                    Sampled profile · index 1–
                    {numeric.data.length.toLocaleString()}
                  </span>
                </div>
              )}
              <p className="qu-note">
                {summary.count.toLocaleString()} finite values
                {summary.missing > 0
                  ? ` · ${summary.missing.toLocaleString()} missing / non-finite`
                  : ""}
              </p>
            </>
          )}
          <div className="qu-preview-label">Value preview</div>
          <pre className="qu-value-preview">{variable.value}</pre>
        </>
      )}
      {numeric && view === "data" && (
        <>
          <div className="qu-data-scroll">
            <table className="qu-data-table">
              <thead>
                <tr>
                  <th scope="col">#</th>
                  {Array.from(
                    { length: Math.min(COLS, columns - colStart) },
                    (_, c) => (
                      <th key={c} scope="col">
                        {columns === 1 ? "Value" : `C${colStart + c + 1}`}
                      </th>
                    ),
                  )}
                </tr>
              </thead>
              <tbody>
                {Array.from(
                  { length: Math.min(ROWS, rows - rowStart) },
                  (_, r) => (
                    <tr key={r}>
                      <th scope="row">{rowStart + r + 1}</th>
                      {Array.from(
                        { length: Math.min(COLS, columns - colStart) },
                        (_, c) => {
                          const value = matrixValue(
                            numeric,
                            rowStart + r,
                            colStart + c,
                          );
                          return (
                            <td
                              key={c}
                              title={
                                value == null
                                  ? "Missing / non-finite"
                                  : String(value)
                              }
                            >
                              {formatNumber(value)}
                            </td>
                          );
                        },
                      )}
                    </tr>
                  ),
                )}
              </tbody>
            </table>
          </div>
          <div className="qu-pagination">
            <span>
              {rows
                ? `Rows ${rowStart + 1}–${Math.min(rows, rowStart + ROWS)} of ${rows.toLocaleString()}`
                : "Empty array"}
            </span>
            <span className="qu-spacer" />
            <button
              className="qu-icon-button"
              aria-label="Previous rows"
              disabled={rowStart === 0}
              onClick={() => setRowPage(rowStart / ROWS - 1)}
            >
              <ChevronLeft size={14} />
            </button>
            <button
              className="qu-icon-button"
              aria-label="Next rows"
              disabled={rowStart + ROWS >= rows}
              onClick={() => setRowPage(rowStart / ROWS + 1)}
            >
              <ChevronRight size={14} />
            </button>
          </div>
          {columns > COLS && (
            <div className="qu-pagination">
              <span>
                Columns {colStart + 1}–{Math.min(columns, colStart + COLS)} of{" "}
                {columns}
              </span>
              <span className="qu-spacer" />
              <button
                className="qu-icon-button"
                aria-label="Previous columns"
                disabled={!colStart}
                onClick={() => setColPage(colStart / COLS - 1)}
              >
                <ChevronLeft size={14} />
              </button>
              <button
                className="qu-icon-button"
                aria-label="Next columns"
                disabled={colStart + COLS >= columns}
                onClick={() => setColPage(colStart / COLS + 1)}
              >
                <ChevronRight size={14} />
              </button>
            </div>
          )}
        </>
      )}
      {numeric && view === "plot" && (
        <>
          <PlotViewer
            height={340}
            title={variable.name}
            subtitle={
              isMatrix
                ? `${rows} rows × ${columns} columns`
                : `${numeric.data.length.toLocaleString()} values · sampled profile`
            }
            theme={theme}
            showLegend={false}
            xlabel={isMatrix ? "Column" : "Index"}
            ylabel={isMatrix ? "Row" : "Value"}
            data={
              isMatrix
                ? {
                    type: "heatmap",
                    x: Array.from(
                      { length: Math.min(columns, 50) },
                      (_, c) => c + 1,
                    ),
                    y: Array.from(
                      { length: Math.min(rows, 50) },
                      (_, r) => r + 1,
                    ),
                    z: Array.from({ length: Math.min(rows, 50) }, (_, r) =>
                      Array.from(
                        { length: Math.min(columns, 50) },
                        (_, c) => matrixValue(numeric, r, c) ?? null,
                      ),
                    ),
                  }
                : {
                    type: "scatter",
                    mode: numeric.data.length === 1 ? "markers" : "lines",
                    x: samples.map((p) => p.x),
                    y: samples.map((p) => p.y),
                  }
            }
          />
          <p className="qu-note">
            {isMatrix
              ? `Preview of ${Math.min(rows, 50)} × ${Math.min(columns, 50)} cells`
              : `Sampled preview · ${numeric.data.length.toLocaleString()} total values`}{" "}
            · CSV includes all values.
          </p>
        </>
      )}
    </section>
  );
}

/** Inspect the most recent run using real numeric data, never parsed previews. */
export const VariableExplorer: React.FC<VariableExplorerProps> = ({
  variables = [],
  numericData = [],
  theme = "dark",
  isRunning = false,
  className,
}) => {
  const [query, setQuery] = useState("");
  const [type, setType] = useState("all");
  const [descending, setDescending] = useState(false);
  const [selected, setSelected] = useState<string | null>(null);
  const [page, setPage] = useState(0);
  const types = useMemo(
    () => [...new Set(variables.map((v) => v.type))].sort(),
    [variables],
  );
  const activeType = types.includes(type) ? type : "all";
  const filtered = useMemo(
    () =>
      variables
        .filter(
          (v) =>
            (activeType === "all" || v.type === activeType) &&
            `${v.name} ${v.type}`
              .toLowerCase()
              .includes(query.toLowerCase().trim()),
        )
        .sort(
          (a, b) =>
            (descending ? -1 : 1) *
            a.name.localeCompare(b.name, undefined, { numeric: true }),
        ),
    [variables, activeType, query, descending],
  );
  const numericByName = useMemo(
    () => new Map(numericData.map((v) => [v.name, v])),
    [numericData],
  );
  const current = filtered.find((v) => v.name === selected);
  const pageStart =
    Math.min(page, Math.max(0, Math.ceil(filtered.length / PAGE_SIZE) - 1)) *
    PAGE_SIZE;
  return (
    <div
      className={cn("qu-inspector qu-variables", className)}
      data-theme={theme}
      aria-label="Variable explorer"
      aria-busy={isRunning}
    >
      <div className="qu-section-header">
        <Database size={15} />
        <h2>Variables</h2>
        <span className="qu-count">{variables.length}</span>
        <span className="qu-spacer" />
        <span className="qu-note">{isRunning ? "Running…" : "Last run"}</span>
      </div>
      {variables.length === 0 ? (
        <div className="qu-empty">
          <Database size={26} />
          <strong>Your workspace, at a glance</strong>
          <p>
            Run a script to inspect values, explore arrays, and preview your
            data.
          </p>
        </div>
      ) : (
        <>
          <div className="qu-variable-controls">
            <label className="qu-search">
              <Search size={14} />
              <input
                aria-label="Search variables"
                placeholder="Find a variable…"
                value={query}
                onChange={(e) => {
                  setQuery(e.target.value);
                  setPage(0);
                }}
              />
              {query && (
                <button
                  className="qu-icon-button"
                  aria-label="Clear variable search"
                  onClick={() => {
                    setQuery("");
                    setPage(0);
                  }}
                >
                  <X size={12} />
                </button>
              )}
            </label>
            <select
              aria-label="Filter by variable type"
              value={activeType}
              onChange={(e) => {
                setType(e.target.value);
                setPage(0);
              }}
            >
              <option value="all">All types</option>
              {types.map((t) => (
                <option key={t}>{t}</option>
              ))}
            </select>
            <button
              className="qu-icon-button"
              aria-label={
                descending
                  ? "Sort variables ascending"
                  : "Sort variables descending"
              }
              title={descending ? "Sort A–Z" : "Sort Z–A"}
              onClick={() => {
                setDescending(!descending);
                setPage(0);
              }}
            >
              {descending ? <ArrowUpAZ size={16} /> : <ArrowDownAZ size={16} />}
            </button>
          </div>
          <div className="qu-variable-columns">
            <span>Name / type</span>
            <span>Value / shape</span>
          </div>
          <div className="qu-variable-list">
            {filtered
              .slice(pageStart, pageStart + PAGE_SIZE)
              .map((variable) => {
                const numeric = numericByName.get(variable.name);
                return (
                  <button
                    key={variable.name}
                    className="qu-variable-row"
                    aria-expanded={selected === variable.name}
                    onClick={() =>
                      setSelected(
                        selected === variable.name ? null : variable.name,
                      )
                    }
                  >
                    <ChevronRight size={13} className="qu-row-chevron" />
                    <span className="qu-variable-name">
                      <strong title={variable.name}>{variable.name}</strong>
                      <span className="qu-type" data-kind={variable.type}>
                        {variable.type}
                      </span>
                    </span>
                    <span className="qu-variable-value">
                      <span title={variable.value}>{variable.value}</span>
                      <small>
                        {numeric
                          ? numeric.shape.join(" × ")
                          : (variable.size ?? "—")}
                      </small>
                    </span>
                  </button>
                );
              })}
            {filtered.length === 0 && (
              <div className="qu-empty">
                <Search size={22} />
                <strong>No matching variables</strong>
                <button
                  className="qu-text-button"
                  onClick={() => {
                    setQuery("");
                    setType("all");
                    setPage(0);
                  }}
                >
                  Clear filters
                </button>
              </div>
            )}
          </div>
          {filtered.length > PAGE_SIZE && (
            <div className="qu-pagination">
              <span>
                {pageStart + 1}–
                {Math.min(pageStart + PAGE_SIZE, filtered.length)} of{" "}
                {filtered.length}
              </span>
              <span className="qu-spacer" />
              <button
                className="qu-icon-button"
                aria-label="Previous variables"
                disabled={!pageStart}
                onClick={() => setPage(pageStart / PAGE_SIZE - 1)}
              >
                <ChevronLeft size={14} />
              </button>
              <button
                className="qu-icon-button"
                aria-label="Next variables"
                disabled={pageStart + PAGE_SIZE >= filtered.length}
                onClick={() => setPage(pageStart / PAGE_SIZE + 1)}
              >
                <ChevronRight size={14} />
              </button>
            </div>
          )}
          {current ? (
            <VariableDetail
              key={current.name}
              variable={current}
              numeric={numericByName.get(current.name)}
              theme={theme}
            />
          ) : (
            filtered.length > 0 && (
              <div className="qu-inspect-hint">
                <ChevronDown size={13} />
                Select a variable to explore its values
              </div>
            )
          )}
        </>
      )}
    </div>
  );
};

export default VariableExplorer;
