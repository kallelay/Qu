import React, { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  Type,
  Square,
  SlidersHorizontal,
  Hash,
  TextCursorInput,
  CheckSquare,
  LineChart,
  LayoutPanelTop,
  ChevronDown,
  Trash2,
  Copy,
  ClipboardPaste,
  Layers,
  FilePlus2,
  Undo2,
  Redo2,
  Search,
  AppWindow,
} from "lucide-react";
import {
  type DesignNode,
  type DesignProps,
  type PropSpec,
  type WidgetKind,
  type NodeKind,
  ROOT_ID,
  PROP_CATEGORIES,
  CONTAINER_KINDS,
  createRoot,
  addNode,
  removeNode,
  moveNode,
  duplicateNode,
  updateProps,
  setOptions,
  setEvent,
  renameNode,
  children,
  eventsFor,
  propsFor,
  defaultHandlerName,
  designTreeToQu,
} from "../utils/guiDesign";

/** A visual designer for Qu's retained GUI runtime (`Frame`/`.add`/`.on`/
 * `.show`, `engine/crates/qu-interp/src/gui.rs`). See
 * `docs/design/qu-gui-designer-spec.md` for the full design: canvas is
 * TREE-based, not freeform, because the runtime only expresses row/
 * column/grid containers.
 *
 * Laid out the way VB6 actually arranges its designer -- Toolbox left,
 * form centre, outline over Properties on the right -- because that
 * arrangement is the thing Ahmed asked for by name, and because the
 * outline is load-bearing here in a way it is not in VB6: reordering and
 * reparenting have no freeform-drag equivalent in this runtime, so the
 * tree is the ONLY place those two operations can live.
 *
 * § Design/Code/Run split (2026-09-16, Ahmed's direct instruction): every
 * visual action regenerates `form.design.qu` (`designTreeToQu`) -- the
 * WIDGET TREE and `.on(...)` bindings ONLY, never handler bodies -- the
 * same "regenerate the machine-owned file wholesale" model VB6/WinForms'
 * own `Designer.cs` uses, and NOT a one-way "draw emits code" dead end:
 * the user's own hand-written handler logic lives in a SEPARATE file,
 * `form.code.qu`, that this component never touches and the host
 * (`GuiDesignerPanel.tsx`) owns. This component still never runs Qu --
 * it hands `form.design.qu`'s content to `onInsertCode`, and the host is
 * responsible for reconciling `form.code.qu`'s handler stubs
 * (`reconcileCodeQu`), merging the two (`mergeGenScript` ->
 * `form.gen.qu`), and actually executing that via `GuiPanel`.
 */

interface PaletteEntry {
  kind: WidgetKind;
  label: string;
  icon: React.ElementType;
  /** What it's for, in the toolbox tooltip and searchable alongside the
   *  label -- "dropdown" should find `select`, "tick box" `checkbox`. */
  blurb: string;
  keywords: string;
}

/** Toolbox groups, in the order VB6's own tray reads: containers first
 *  (you place those before what goes in them), then static display, then
 *  the input controls, then the one data-bound widget. */
const PALETTE: Array<{ group: string; entries: PaletteEntry[] }> = [
  {
    group: "Containers",
    entries: [
      { kind: "panel", label: "Panel", icon: LayoutPanelTop, blurb: "A row/column/grid group for other widgets", keywords: "container group box layout" },
    ],
  },
  {
    group: "Display",
    entries: [
      { kind: "label", label: "Label", icon: Type, blurb: "Static caption text", keywords: "caption static text title" },
    ],
  },
  {
    group: "Input",
    entries: [
      { kind: "button", label: "Button", icon: Square, blurb: "Push button, raises click", keywords: "command push press click" },
      { kind: "slider", label: "Slider", icon: SlidersHorizontal, blurb: "Drag a value between min and max", keywords: "range track scroll seek" },
      { kind: "number", label: "Number", icon: Hash, blurb: "Typed numeric entry with bounds", keywords: "spin numeric int float" },
      { kind: "text", label: "Text box", icon: TextCursorInput, blurb: "Single-line text entry", keywords: "textbox string input field edit" },
      { kind: "checkbox", label: "Check box", icon: CheckSquare, blurb: "On/off toggle", keywords: "toggle tick boolean switch" },
      { kind: "select", label: "Dropdown", icon: ChevronDown, blurb: "Pick one of a fixed list", keywords: "dropdown combo list choice option" },
    ],
  },
  {
    group: "Data",
    entries: [
      { kind: "plot", label: "Plot", icon: LineChart, blurb: "Live chart bound to script variables", keywords: "chart graph figure curve" },
    ],
  },
];

const KIND_ICON: Record<NodeKind, React.ElementType> = {
  frame: AppWindow,
  panel: LayoutPanelTop,
  label: Type,
  button: Square,
  slider: SlidersHorizontal,
  number: Hash,
  text: TextCursorInput,
  checkbox: CheckSquare,
  select: ChevronDown,
  plot: LineChart,
};

const border = "1px solid var(--qu-border)";
/** Drop indicators, selection outlines, resize handles, the bound-event
 *  markers -- every HAIRLINE the designer draws over its own canvas. It
 *  was a fixed `#2a78d6` in both themes, which is 3.1:1 against the dark
 *  canvas: a 2px drop bar you have to look for is not a drop bar. The
 *  token lightens in dark mode and stays the same blue in light. */
const ACCENT = "var(--qu-accent)";
/** Accent FILL behind white text (the selected outline row). Distinct
 *  from ACCENT above for the reason spelled out in inspector.css: one
 *  value cannot both sit on the page and sit under white type. */
const ACCENT_SOLID = "var(--qu-accent-solid)";
const muted = "var(--qu-muted)";

export interface GuiDesignerProps {
  theme?: "light" | "dark";
  /** Insert/replace the generated source in the paired editor. Absent ->
   *  the "Insert into script" button is hidden, same convention
   *  `FigureViewer`'s `onInsertCode` already uses. */
  onInsertCode?: (code: string) => void;
  /** Fired with `form.design.qu`'s content every time the design tree
   *  changes, including once on mount.
   *
   *  `onInsertCode` alone is not enough for a host that has to RUN the
   *  design: it only fires when the user clicks the code strip's sync
   *  button, so a host holding design source in state had nothing but an
   *  empty string until that click -- and "draw a form, press Run" is the
   *  obvious path that never touches it. See `GuiDesignerPanel.tsx`.
   *  `onInsertCode` remains the explicit "write it to disk now" action;
   *  this is the passive mirror of the current tree. */
  onDesignCodeChange?: (code: string) => void;
}

// Persisted the same way `GuiPanel` persists its own source
// (`qu-gui-source`): this panel is one of several sub-tabs/host tabs a
// person switches away from and back to constantly (see
// `GuiDesignerPanel.tsx`'s Design/Run pair), and each of those switches
// unmounts this component -- verified live, switching to Run and back
// wiped an in-progress design before this was added.
const STORAGE_KEY = "qu-gui-designer-tree";

function loadSavedTree(): DesignNode[] {
  try {
    const saved = localStorage.getItem(STORAGE_KEY);
    if (saved) {
      const parsed = JSON.parse(saved);
      if (Array.isArray(parsed) && parsed.some((n) => n?.id === ROOT_ID)) return parsed as DesignNode[];
    }
  } catch {
    // Corrupt or foreign localStorage content -- fall through to a fresh
    // design rather than crash the panel on mount.
  }
  return [createRoot()];
}

interface HistoryState {
  past: DesignNode[][];
  present: DesignNode[];
  future: DesignNode[][];
}

/** Undo/redo over the DESIGN TREE, not over the generated text. The
 *  previous build had undo only via the code textarea, which meant every
 *  visual action -- the ones people actually take here -- was
 *  irreversible. Capped so a long session can't grow the stack without
 *  bound. A commit that returns the same array reference (every `guiDesign`
 *  helper no-ops that way when it refuses an edit) records no history
 *  entry, so a rejected rename can't leave an undo step that does nothing. */
function useDesignHistory(initial: () => DesignNode[]) {
  const [state, setState] = useState<HistoryState>(() => ({ past: [], present: initial(), future: [] }));
  // `latest` is the committed tree as of RIGHT NOW, not as of the last
  // render. Actions need it because they must both derive a new tree and
  // select the node they just created: reading `state.present` from the
  // render closure loses every edit but the last when two actions fire in
  // one tick, and doing the work inside a `setState` updater instead makes
  // the updater impure -- which React's StrictMode double-invokes,
  // running `addNode`'s id generation twice per placement.
  const latest = useRef(state.present);
  latest.current = state.present;
  const commit = useCallback((next: DesignNode[] | ((prev: DesignNode[]) => DesignNode[])) => {
    const resolved = typeof next === "function" ? next(latest.current) : next;
    if (resolved === latest.current) return;
    latest.current = resolved;
    // A new edit invalidates the redo branch, as everywhere else.
    setState((s) => ({ past: [...s.past, s.present].slice(-80), present: resolved, future: [] }));
  }, []);
  // These read the past/future stacks, which only `state` holds, so they
  // stay updaters. Re-pointing `latest` from inside one is idempotent (it
  // assigns the same tree however many times StrictMode re-runs it), which
  // is what makes it safe here and is not true of creating nodes.
  const undo = useCallback(() => {
    setState((s) => {
      if (!s.past.length) return s;
      latest.current = s.past[s.past.length - 1];
      return { past: s.past.slice(0, -1), present: latest.current, future: [s.present, ...s.future].slice(0, 80) };
    });
  }, []);
  const redo = useCallback(() => {
    setState((s) => {
      if (!s.future.length) return s;
      latest.current = s.future[0];
      return { past: [...s.past, s.present].slice(-80), present: latest.current, future: s.future.slice(1) };
    });
  }, []);
  return { nodes: state.present, latest, commit, undo, redo, canUndo: state.past.length > 0, canRedo: state.future.length > 0 };
}

type DropWhere = "before" | "after" | "into";
interface DropHint {
  id: string;
  where: DropWhere;
}
/** What is being dragged: an existing node, or a not-yet-created widget
 *  from the toolbox. Held in a ref rather than `dataTransfer` because the
 *  payload is structured and only ever travels within this component --
 *  `dataTransfer` still gets a marker string so the browser permits drops. */
type DragPayload = { type: "node"; id: string } | { type: "new"; kind: WidgetKind };

export const GuiDesigner: React.FC<GuiDesignerProps> = ({ theme = "light", onInsertCode, onDesignCodeChange }) => {
  const { nodes, latest, commit, undo, redo, canUndo, canRedo } = useDesignHistory(loadSavedTree);
  const [selection, setSelection] = useState<string[]>([ROOT_ID]);
  // The container a toolbox click inserts into. Defaults to the root and
  // follows selection whenever a container is selected -- matches "click
  // a panel, then add controls to it" as the natural flow. Mirrored into a
  // ref for the same reason the tree is: placing a panel and then a widget
  // inside it are two actions that can land in one tick, and the second
  // must see the container the first just created, not the render-closure
  // value from before either happened.
  const [containerId, setContainerIdState] = useState<string>(ROOT_ID);
  const container = useRef(ROOT_ID);
  const setContainerId = useCallback((id: string) => {
    container.current = id;
    setContainerIdState(id);
  }, []);
  const [query, setQuery] = useState("");
  const [byCategory, setByCategory] = useState(true);
  const [renamingId, setRenamingId] = useState<string | null>(null);
  const [clipboard, setClipboard] = useState<DesignNode[] | null>(null);
  const [dropHint, setDropHint] = useState<DropHint | null>(null);
  const drag = useRef<DragPayload | null>(null);
  const surface = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify(nodes));
    } catch {
      // Best-effort; a full/blocked localStorage should not break designing.
    }
  }, [nodes]);

  const code = useMemo(() => designTreeToQu(nodes).join("\n"), [nodes]);
  // Mirror the current design out to the host on every change (and on
  // mount, which is what makes a restored-from-localStorage tree visible
  // to a host that has not yet seen a single edit). Kept in a ref so a
  // host passing an inline arrow does not re-fire this on every render.
  const designCodeSink = useRef(onDesignCodeChange);
  designCodeSink.current = onDesignCodeChange;
  useEffect(() => { designCodeSink.current?.(code); }, [code]);
  const byId = useMemo(() => new Map(nodes.map((n) => [n.id, n])), [nodes]);
  const selected = useMemo(() => selection.map((id) => byId.get(id)).filter((n): n is DesignNode => !!n), [selection, byId]);
  const primary = selected[selected.length - 1] ?? null;

  /** Depth-first visible order -- what Tab and the arrow keys walk, and
   *  the order the outline paints. */
  const flat = useMemo(() => {
    const out: Array<{ node: DesignNode; depth: number }> = [];
    const walk = (id: string, depth: number) => {
      const node = byId.get(id);
      if (!node) return;
      out.push({ node, depth });
      for (const child of children(nodes, id)) walk(child.id, depth + 1);
    };
    walk(ROOT_ID, 0);
    return out;
  }, [nodes, byId]);

  const select = useCallback(
    (node: DesignNode, additive = false) => {
      setSelection((prev) => {
        if (!additive) return [node.id];
        // Shift-click a selected node to drop it, except the last one: an
        // empty selection would leave the Properties window with nothing to
        // show and no obvious way back.
        if (prev.includes(node.id)) return prev.length > 1 ? prev.filter((id) => id !== node.id) : prev;
        return [...prev, node.id];
      });
      if (CONTAINER_KINDS.includes(node.kind)) setContainerId(node.id);
    },
    [setContainerId],
  );

  const place = useCallback(
    (kind: WidgetKind, parentId?: string, beforeId: string | null = null) => {
      const target = parentId ?? container.current;
      const grown = addNode(latest.current, target, kind);
      if (grown === latest.current) return;
      const added = grown[grown.length - 1];
      commit(beforeId ? moveNode(grown, added.id, target, beforeId) : grown);
      setSelection([added.id]);
      if (CONTAINER_KINDS.includes(added.kind)) setContainerId(added.id);
    },
    [commit, latest, setContainerId],
  );

  /** Deleting can orphan BOTH the selection and the insertion point (a
   *  removed container takes its descendants with it), and a stale
   *  `containerId` makes every later toolbox click silently no-op with
   *  nothing on screen explaining why. Re-check both against the surviving
   *  tree rather than only the ids that were asked for. */
  const removeSelected = useCallback(() => {
    const before = latest.current;
    const next = selection.filter((id) => id !== ROOT_ID).reduce((acc, id) => removeNode(acc, id), before);
    if (next === before) return;
    commit(next);
    const alive = new Set(next.map((n) => n.id));
    setSelection((sel) => (sel.some((id) => alive.has(id)) ? sel.filter((id) => alive.has(id)) : [ROOT_ID]));
    if (!alive.has(container.current)) setContainerId(ROOT_ID);
  }, [commit, latest, selection, setContainerId]);

  const duplicateSelected = useCallback(() => {
    let next = latest.current;
    const fresh: string[] = [];
    for (const id of selection) {
      const result = duplicateNode(next, id);
      next = result.nodes;
      if (result.newId) fresh.push(result.newId);
    }
    if (next === latest.current) return;
    commit(next);
    if (fresh.length) setSelection(fresh);
  }, [commit, latest, selection]);

  /** Copy keeps a detached snapshot of the subtree rather than an id, so
   *  it survives deleting the original -- the case where paste is most
   *  wanted. Paste re-creates it through `addNode`/`updateProps` so every
   *  copy gets fresh ids and fresh `{kind}{n}` names from the CURRENT
   *  tree; replaying the stored nodes verbatim would collide. */
  const copySelected = useCallback(() => {
    const roots = selected.filter((n) => n.id !== ROOT_ID);
    setClipboard(roots.length ? roots.map((n) => ({ ...n, props: { ...n.props } })) : null);
  }, [selected]);

  const paste = useCallback(() => {
    if (!clipboard?.length) return;
    let next = latest.current;
    const fresh: string[] = [];
    for (const source of clipboard) {
      const host = next.find((n) => n.id === container.current);
      const target = host && CONTAINER_KINDS.includes(host.kind) ? host.id : ROOT_ID;
      const grown = addNode(next, target, source.kind as WidgetKind);
      if (grown === next) continue;
      const added = grown[grown.length - 1];
      next = updateProps(grown, added.id, { ...source.props });
      for (const [event, handler] of Object.entries(source.events)) {
        if (handler) next = setEvent(next, added.id, event as "click" | "change" | "close", handler);
      }
      fresh.push(added.id);
    }
    if (next === latest.current) return;
    commit(next);
    if (fresh.length) setSelection(fresh);
  }, [clipboard, commit, latest]);

  /** Double-click toggles the widget's PRIMARY event (its first entry in
   *  `eventsFor`) on and off, generating `{var}_{event}` and a matching
   *  handler stub -- the VB6/App-Designer "double-click for a callback"
   *  interaction from the spec, simplified for v1: it ensures the binding
   *  and stub exist in the generated code rather than moving a cursor
   *  inside a live Monaco instance (this component doesn't own one). */
  const toggleDefaultEvent = useCallback(
    (node: DesignNode) => {
      const [event] = eventsFor(node.kind);
      if (!event) return;
      commit((prev) => setEvent(prev, node.id, event, node.events[event] ? null : defaultHandlerName(node, event)));
    },
    [commit],
  );

  /** Move the primary selection one slot within its parent. The keyboard
   *  equivalent of a drag in the outline; VB6's arrow keys nudge a control
   *  by pixels, which has no meaning in a flex/grid-only runtime, so they
   *  nudge it through sibling ORDER instead -- the thing that does. */
  const nudge = useCallback(
    (delta: -1 | 1) => {
      if (!primary || primary.id === ROOT_ID || !primary.parentId) return;
      const siblings = children(nodes, primary.parentId);
      const at = siblings.findIndex((n) => n.id === primary.id);
      const to = at + delta;
      if (at < 0 || to < 0 || to >= siblings.length) return;
      // Moving down past a sibling means landing before the one AFTER it.
      const beforeId = delta < 0 ? siblings[to].id : (siblings[to + 1]?.id ?? null);
      commit((prev) => moveNode(prev, primary.id, primary.parentId!, beforeId));
    },
    [commit, nodes, primary],
  );

  const step = useCallback(
    (delta: -1 | 1) => {
      const at = flat.findIndex(({ node }) => node.id === primary?.id);
      const next = flat[(at + delta + flat.length) % flat.length];
      if (next) select(next.node);
    },
    [flat, primary, select],
  );

  const onKeyDown = (event: React.KeyboardEvent) => {
    // Never steal a key from a field being typed into -- the inspector and
    // the rename box are both inside this container's focus subtree.
    const target = event.target as HTMLElement;
    if (/^(INPUT|TEXTAREA|SELECT)$/.test(target.tagName) || target.isContentEditable) return;
    const mod = event.ctrlKey || event.metaKey;
    const handlers: Record<string, () => void> = {
      Delete: removeSelected,
      Backspace: removeSelected,
      F2: () => primary && setRenamingId(primary.id),
      Tab: () => step(event.shiftKey ? -1 : 1),
      ArrowDown: () => (mod ? nudge(1) : step(1)),
      ArrowUp: () => (mod ? nudge(-1) : step(-1)),
    };
    const combo = mod ? { z: event.shiftKey ? redo : undo, y: redo, d: duplicateSelected, c: copySelected, v: paste }[event.key.toLowerCase()] : undefined;
    const run = combo ?? handlers[event.key];
    if (!run) return;
    event.preventDefault();
    run();
  };

  // --- drag and drop -------------------------------------------------
  // One drop model for the outline and the canvas: the top/bottom quarter
  // of a row means "as a sibling before/after it", the middle means "into
  // it" for a container. A non-container's middle falls back to sibling
  // placement, so dragging onto a button can never produce a parent
  // `gui.rs` would reject ("Only frames and panels can contain widgets").
  const hintFrom = (event: React.DragEvent, node: DesignNode): DropHint => {
    const box = event.currentTarget.getBoundingClientRect();
    const ratio = (event.clientY - box.top) / Math.max(box.height, 1);
    const container = CONTAINER_KINDS.includes(node.kind);
    if (node.id === ROOT_ID) return { id: node.id, where: "into" };
    if (container && ratio > 0.25 && ratio < 0.75) return { id: node.id, where: "into" };
    return { id: node.id, where: ratio < 0.5 ? "before" : "after" };
  };

  const applyDrop = (hint: DropHint) => {
    const target = byId.get(hint.id);
    const payload = drag.current;
    if (!target || !payload) return;
    let parentId = target.id;
    let beforeId: string | null = null;
    if (hint.where !== "into") {
      parentId = target.parentId ?? ROOT_ID;
      const siblings = children(nodes, parentId);
      const at = siblings.findIndex((n) => n.id === target.id);
      beforeId = hint.where === "before" ? target.id : (siblings[at + 1]?.id ?? null);
    }
    if (payload.type === "new") place(payload.kind, parentId, beforeId);
    else commit((prev) => moveNode(prev, payload.id, parentId, beforeId));
  };

  const dropProps = (node: DesignNode) => ({
    onDragOver: (event: React.DragEvent) => {
      if (!drag.current) return;
      event.preventDefault();
      event.stopPropagation();
      setDropHint(hintFrom(event, node));
    },
    onDrop: (event: React.DragEvent) => {
      event.preventDefault();
      event.stopPropagation();
      const hint = hintFrom(event, node);
      applyDrop(hint);
      drag.current = null;
      setDropHint(null);
    },
  });

  const dragProps = (node: DesignNode) =>
    node.id === ROOT_ID
      ? {}
      : {
          draggable: true,
          onDragStart: (event: React.DragEvent) => {
            drag.current = { type: "node", id: node.id };
            event.dataTransfer.effectAllowed = "move";
            event.dataTransfer.setData("text/plain", node.varName);
          },
          onDragEnd: () => {
            drag.current = null;
            setDropHint(null);
          },
        };

  const root = byId.get(ROOT_ID) ?? nodes[0];
  // Surfaces come from the theme tokens now, so nothing in here needs to
  // branch on `theme` by hand any more.
  const surfaceBg = "var(--qu-bg)";

  // --- canvas --------------------------------------------------------
  /** WYSIWYG preview. Deliberately mirrors `GuiPanel.tsx`'s own render
   *  switch (flex direction from `layout`, grid via auto-fit columns) so
   *  the design surface and the real runtime agree about what a container
   *  does -- the two switches are the pair the spec warns must stay in
   *  lockstep. What it does NOT do is run Qu: a plot shows its bound
   *  expression, not data, because the data lives in script variables this
   *  component cannot see. */
  const renderPreview = (node: DesignNode): React.ReactNode => {
    const isContainer = CONTAINER_KINDS.includes(node.kind);
    const kids = children(nodes, node.id);
    const isSelected = selection.includes(node.id);
    const isDrop = isContainer && node.id === containerId;
    const hint = dropHint?.id === node.id ? dropHint.where : null;
    const p = node.props;
    const dim = p.visible === false || p.disabled;
    const bar = (side: "before" | "after") => hint === side && (
      <div style={{ height: 3, background: ACCENT, borderRadius: 2, margin: "1px 0" }} />
    );

    const inner = (() => {
      if (isContainer) {
        return (
          <div
            style={{
              display: p.layout === "grid" ? "grid" : "flex",
              flexDirection: p.layout === "row" ? "row" : "column",
              gridTemplateColumns: p.layout === "grid" ? "repeat(auto-fit, minmax(140px, 1fr))" : undefined,
              gap: 10,
              flexWrap: "wrap",
              alignItems: "stretch",
              minHeight: 40,
              padding: 8,
              border: kids.length ? "1px dashed transparent" : `1px dashed ${isDrop ? ACCENT : "var(--qu-border)"}`,
              borderRadius: 6,
              fontSize: 11,
              color: muted,
            }}
          >
            {kids.length ? kids.map(renderPreview) : `Empty ${node.kind} — drop a widget here, or select it and click one in the Toolbox`}
          </div>
        );
      }
      switch (node.kind) {
        case "label":
          return <span style={{ fontSize: 13 }}>{p.text || "Label"}</span>;
        case "button":
          return <button type="button" tabIndex={-1} style={{ padding: "4px 12px", border, borderRadius: 5, background: "var(--qu-hover)", fontSize: 12 }}>{p.text || "Button"}</button>;
        case "slider":
          return (
            <label style={{ display: "flex", flexDirection: "column", gap: 2, fontSize: 11 }}>
              {p.text}
              <input type="range" tabIndex={-1} readOnly min={p.min ?? 0} max={p.max ?? 100} value={typeof p.value === "number" ? p.value : 0} style={{ width: "100%" }} />
            </label>
          );
        case "number":
          return (
            <label style={{ display: "flex", flexDirection: "column", gap: 2, fontSize: 11 }}>
              {p.text}
              <input type="number" tabIndex={-1} readOnly value={typeof p.value === "number" ? p.value : 0} style={{ width: 80, padding: "3px 5px", border, borderRadius: 4 }} />
            </label>
          );
        case "text":
          return (
            <label style={{ display: "flex", flexDirection: "column", gap: 2, fontSize: 11 }}>
              {p.text}
              <input tabIndex={-1} readOnly value={typeof p.value === "string" ? p.value : ""} style={{ padding: "3px 5px", border, borderRadius: 4 }} />
            </label>
          );
        case "checkbox":
          return (
            <label style={{ display: "flex", alignItems: "center", gap: 5, fontSize: 12 }}>
              <input type="checkbox" tabIndex={-1} readOnly checked={p.value === true} />
              {p.text || "Checkbox"}
            </label>
          );
        case "select":
          return (
            <label style={{ display: "flex", flexDirection: "column", gap: 2, fontSize: 11 }}>
              {p.text}
              <select tabIndex={-1} value={typeof p.value === "string" ? p.value : ""} onChange={() => undefined} style={{ padding: "3px 5px", border, borderRadius: 4, fontSize: 12 }}>
                {(p.options ?? []).map((option) => (
                  <option key={option} value={option}>{option}</option>
                ))}
              </select>
            </label>
          );
        case "plot":
          // A placeholder, not a chart: `x`/`y` are Qu expressions
          // evaluated by the engine at run time (spec §4), so there is
          // nothing here to draw until Run.
          return (
            <div style={{ border, borderRadius: 6, padding: 10, background: "var(--qu-surface)", minHeight: 90, display: "flex", flexDirection: "column", justifyContent: "center", alignItems: "center", gap: 4 }}>
              <LineChart size={20} color={muted} />
              <strong style={{ fontSize: 12 }}>{p.title || "Plot"}</strong>
              <code style={{ fontSize: 10, color: muted }}>y = {p.y || "?"} vs x = {p.x || "?"}</code>
            </div>
          );
        default:
          return <span style={{ fontSize: 12 }}>{node.kind}</span>;
      }
    })();

    return (
      <div key={node.id} style={{ minWidth: 0, flex: node.kind === "plot" ? "1 1 100%" : undefined }}>
        {bar("before")}
        <div
          role="button"
          tabIndex={-1}
          aria-label={`${node.kind} ${node.varName}`}
          aria-pressed={isSelected}
          onClick={(event) => {
            event.stopPropagation();
            select(node, event.shiftKey);
          }}
          onDoubleClick={(event) => {
            event.stopPropagation();
            toggleDefaultEvent(node);
          }}
          {...dragProps(node)}
          {...dropProps(node)}
          style={{
            position: "relative",
            padding: 4,
            borderRadius: 4,
            outline: isSelected ? `1.5px solid ${ACCENT}` : hint === "into" ? `1.5px dashed ${ACCENT}` : "1.5px solid transparent",
            outlineOffset: 1,
            opacity: dim ? 0.45 : 1,
            cursor: "pointer",
          }}
        >
          {/* A design-canvas preview should never itself be operable, and a
              live native control (range thumb, select) swallows the mouse
              gesture that would otherwise start this wrapper's HTML5 drag --
              `readonly` doesn't stop that; it's a no-op for range/select/
              checkbox. `pointerEvents: none` removes the inner control from
              hit-testing entirely so press-and-drag from anywhere on a
              widget's preview reaches this draggable wrapper.
              Containers are excluded: `pointer-events` is inherited, so
              disabling it on a container's own children-holding div would
              cascade to every nested widget's wrapper too -- those already
              protect themselves individually when `renderPreview` recurses
              into them. */}
          {isContainer ? inner : <div style={{ pointerEvents: "none" }}>{inner}</div>}
          {isSelected && <SelectionHandles />}
        </div>
        {bar("after")}
      </div>
    );
  };

  return (
    <div
      ref={surface}
      tabIndex={0}
      onKeyDown={onKeyDown}
      style={{ display: "flex", height: "100%", minHeight: 0, outline: "none" }}
      data-theme={theme}
    >
      {/* Toolbox */}
      <aside style={{ width: 180, borderRight: border, display: "flex", flexDirection: "column", minHeight: 0 }}>
        <div style={{ padding: "10px 10px 6px" }}>
          <h3 style={{ fontSize: 11, textTransform: "uppercase", color: muted, marginBottom: 6, letterSpacing: 0.4 }}>Toolbox</h3>
          <div style={{ display: "flex", alignItems: "center", gap: 5, border, borderRadius: 5, padding: "3px 6px" }}>
            <Search size={11} color={muted} />
            <input
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              placeholder="Search widgets"
              aria-label="Search widgets"
              style={{ border: "none", outline: "none", background: "transparent", fontSize: 11, width: "100%" }}
            />
          </div>
          <p style={{ fontSize: 10, color: muted, margin: "6px 0 0" }}>
            Click to add to <strong>{byId.get(containerId)?.varName ?? root.varName}</strong>, or drag onto the form.
          </p>
        </div>
        <div style={{ flex: 1, overflowY: "auto", padding: "0 10px 10px" }}>
          {PALETTE.map(({ group, entries }) => {
            const needle = query.trim().toLowerCase();
            const shown = needle
              ? entries.filter((e) => `${e.label} ${e.kind} ${e.keywords} ${e.blurb}`.toLowerCase().includes(needle))
              : entries;
            if (!shown.length) return null;
            return (
              <section key={group} style={{ marginTop: 10 }}>
                <h4 style={{ fontSize: 10, textTransform: "uppercase", color: muted, margin: "0 0 4px", letterSpacing: 0.3 }}>{group}</h4>
                <div style={{ display: "flex", flexDirection: "column", gap: 4 }}>
                  {shown.map(({ kind, label, icon: Icon, blurb }) => (
                    <button
                      key={kind}
                      title={blurb}
                      onClick={() => place(kind)}
                      draggable
                      onDragStart={(event) => {
                        drag.current = { type: "new", kind };
                        event.dataTransfer.effectAllowed = "copy";
                        event.dataTransfer.setData("text/plain", kind);
                      }}
                      onDragEnd={() => {
                        drag.current = null;
                        setDropHint(null);
                      }}
                      style={{ display: "flex", alignItems: "center", gap: 7, padding: "5px 7px", border, borderRadius: 5, background: "transparent", textAlign: "left", fontSize: 12, cursor: "grab" }}
                    >
                      <Icon size={13} />
                      {label}
                    </button>
                  ))}
                </div>
              </section>
            );
          })}
          {query.trim() && !PALETTE.some(({ entries }) => entries.some((e) => `${e.label} ${e.kind} ${e.keywords} ${e.blurb}`.toLowerCase().includes(query.trim().toLowerCase()))) && (
            <p style={{ fontSize: 11, color: muted, marginTop: 10 }}>No widget matches “{query.trim()}”.</p>
          )}
        </div>
      </aside>

      {/* Form surface */}
      <main style={{ flex: 1, minWidth: 0, display: "flex", flexDirection: "column", minHeight: 0 }}>
        <Toolbar
          canUndo={canUndo}
          canRedo={canRedo}
          hasClipboard={!!clipboard?.length}
          canEditSelection={selection.some((id) => id !== ROOT_ID)}
          onUndo={undo}
          onRedo={redo}
          onCopy={copySelected}
          onPaste={paste}
          onDuplicate={duplicateSelected}
          onDelete={removeSelected}
        />
        {/* The canvas is the MAT the form sits on, so it has to be the
              deeper plane: on --qu-surface it came out lighter than the
              form itself in dark mode, which inverts the depth cue and
              makes the form read as a hole rather than a window. */}
          <div style={{ flex: 1, overflow: "auto", padding: 20, background: "var(--qu-shell-sunken)" }} onClick={() => select(root)}>
          {/* The form, as a window: a title bar is what makes the canvas
              read as a form rather than a div, and it shows the `title`
              the generated `Frame(...)` actually carries.
              `docs/design/retro-window-chrome.md` pairs this bar with
              three traffic-light dots, and they are deliberately NOT used
              here -- Ahmed had them removed from the TabBar and Terminal
              on 2026-09-16 ("remove these", board2 quworkspace-a6), and a
              ruling on the chrome outranks the older style reference. */}
          <div style={{ borderRadius: 8, overflow: "hidden", boxShadow: "0 2px 12px rgb(0 0 0 / 18%)", minWidth: 0, maxWidth: 760, margin: "0 auto", background: surfaceBg }}>
            <div
              onClick={(event) => {
                event.stopPropagation();
                select(root);
              }}
              onDoubleClick={(event) => {
                event.stopPropagation();
                toggleDefaultEvent(root);
              }}
              style={{
                display: "flex",
                alignItems: "center",
                gap: 6,
                padding: "7px 10px",
                /* The title bar used to be a fixed light gradient with
                   fixed slate text, in BOTH themes -- so in dark mode the
                   form wore a bright cool-grey cap on a near-black body,
                   the one part of the canvas that did not know the theme
                   had changed. It still reads as window chrome (a step
                   lighter than the form surface under it, a monospace
                   title, a window glyph); it just does so in whichever
                   theme is on. */
                background: "var(--qu-hover)",
                borderBottom: `1px solid ${selection.includes(ROOT_ID) ? ACCENT : "var(--qu-border)"}`,
                cursor: "pointer",
              }}
            >
              <AppWindow size={12} color="var(--qu-muted)" />
              <span style={{ fontFamily: "monospace", fontSize: 11, color: "var(--qu-muted)" }}>{root.props.title || "My Qu app"}</span>
              {root.events.close && <span style={{ marginLeft: "auto", fontSize: 10, color: ACCENT }}>on close → {root.events.close}</span>}
            </div>
            <div
              {...dropProps(root)}
              style={{ padding: 14, minHeight: 200, outline: selection.includes(ROOT_ID) ? `1.5px solid ${ACCENT}` : "none", outlineOffset: -1 }}
            >
              <div
                style={{
                  display: root.props.layout === "grid" ? "grid" : "flex",
                  flexDirection: root.props.layout === "row" ? "row" : "column",
                  gridTemplateColumns: root.props.layout === "grid" ? "repeat(auto-fit, minmax(140px, 1fr))" : undefined,
                  gap: 10,
                  flexWrap: "wrap",
                  alignItems: "stretch",
                }}
              >
                {children(nodes, ROOT_ID).map(renderPreview)}
              </div>
              {!children(nodes, ROOT_ID).length && (
                <p style={{ fontSize: 11, color: muted, textAlign: "center", padding: 24 }}>
                  Empty form — drag a widget here from the Toolbox.
                </p>
              )}
            </div>
          </div>
          <p style={{ fontSize: 10, color: muted, textAlign: "center", marginTop: 12, maxWidth: 760, marginLeft: "auto", marginRight: "auto" }}>
            Widgets flow in row / column / grid containers — Qu&apos;s GUI runtime has no freeform x/y placement, so
            position is sibling order, set by dragging in the form or the outline. Hollow handles mean the container
            governs the size. Double-click a widget for its event handler.
          </p>
        </div>
        <CodeStrip code={code} onInsertCode={onInsertCode} />
      </main>

      {/* Outline over Properties, the VB6 right-hand column */}
      <aside style={{ width: 312, borderLeft: border, display: "flex", flexDirection: "column", minHeight: 0 }}>
        <div style={{ borderBottom: border, display: "flex", flexDirection: "column", maxHeight: "38%", minHeight: 120 }}>
          <h3 style={{ fontSize: 11, textTransform: "uppercase", color: muted, padding: "10px 10px 6px", letterSpacing: 0.4 }}>Outline</h3>
          <div style={{ flex: 1, overflowY: "auto", padding: "0 6px 8px" }}>
            {flat.map(({ node, depth }) => (
              <OutlineRow
                key={node.id}
                node={node}
                depth={depth}
                selected={selection.includes(node.id)}
                isInsertPoint={node.id === containerId}
                hint={dropHint?.id === node.id ? dropHint.where : null}
                renaming={renamingId === node.id}
                onRename={(name) => {
                  if (name) commit((prev) => renameNode(prev, node.id, name));
                  setRenamingId(null);
                }}
                onStartRename={() => setRenamingId(node.id)}
                onSelect={(additive) => select(node, additive)}
                dragProps={dragProps(node)}
                dropProps={dropProps(node)}
              />
            ))}
          </div>
        </div>
        <div style={{ flex: 1, overflowY: "auto", padding: 10, minHeight: 0 }}>
          <div style={{ display: "flex", alignItems: "center", gap: 6, marginBottom: 8 }}>
            <h3 style={{ fontSize: 11, textTransform: "uppercase", color: muted, letterSpacing: 0.4 }}>Properties</h3>
            <button
              onClick={() => setByCategory((v) => !v)}
              title={byCategory ? "Sort alphabetically" : "Group by category"}
              style={{ marginLeft: "auto", border, borderRadius: 4, background: "transparent", fontSize: 10, padding: "2px 6px", cursor: "pointer" }}
            >
              {byCategory ? "A→Z" : "Categorized"}
            </button>
          </div>
          {primary ? (
            <Inspector
              nodes={nodes}
              selected={selected}
              primary={primary}
              byCategory={byCategory}
              commit={commit}
              onStartRename={() => setRenamingId(primary.id)}
            />
          ) : (
            <p style={{ fontSize: 11, color: muted }}>Nothing selected.</p>
          )}
        </div>
      </aside>
    </div>
  );
};

/** VB6 draws HOLLOW handles when a control's size is governed by its
 *  container and solid ones when you can drag it. Every widget here is in
 *  that first case -- row/column/grid layout owns sizing, and there is no
 *  runtime prop to write a dragged size into -- so the handles are drawn
 *  hollow and inert on purpose. Drawing solid, draggable-looking handles
 *  would promise a resize the engine has no way to express. */
const SelectionHandles: React.FC = () => {
  const spots: Array<React.CSSProperties> = [
    { top: -3, left: -3 }, { top: -3, left: "50%", marginLeft: -3 }, { top: -3, right: -3 },
    { top: "50%", left: -3, marginTop: -3 }, { top: "50%", right: -3, marginTop: -3 },
    { bottom: -3, left: -3 }, { bottom: -3, left: "50%", marginLeft: -3 }, { bottom: -3, right: -3 },
  ];
  return (
    <>
      {spots.map((spot, index) => (
        <span
          key={index}
          aria-hidden
          title="Size is governed by the container's layout — Qu's GUI runtime has no width/height property"
          style={{ position: "absolute", width: 6, height: 6, background: "var(--qu-bg)", border: `1px solid ${ACCENT}`, pointerEvents: "none", ...spot }}
        />
      ))}
    </>
  );
};

const Toolbar: React.FC<{
  canUndo: boolean;
  canRedo: boolean;
  hasClipboard: boolean;
  canEditSelection: boolean;
  onUndo: () => void;
  onRedo: () => void;
  onCopy: () => void;
  onPaste: () => void;
  onDuplicate: () => void;
  onDelete: () => void;
}> = ({ canUndo, canRedo, hasClipboard, canEditSelection, onUndo, onRedo, onCopy, onPaste, onDuplicate, onDelete }) => {
  const item = (label: string, Icon: React.ElementType, enabled: boolean, onClick: () => void) => (
    <button
      title={label}
      aria-label={label}
      disabled={!enabled}
      onClick={onClick}
      style={{ display: "flex", alignItems: "center", gap: 4, border: "none", background: "transparent", cursor: enabled ? "pointer" : "default", opacity: enabled ? 1 : 0.35, padding: "3px 5px", fontSize: 11 }}
    >
      <Icon size={13} />
    </button>
  );
  return (
    <div style={{ display: "flex", alignItems: "center", gap: 2, padding: "5px 10px", borderBottom: border }}>
      {item("Undo (Ctrl+Z)", Undo2, canUndo, onUndo)}
      {item("Redo (Ctrl+Shift+Z)", Redo2, canRedo, onRedo)}
      <span style={{ width: 1, height: 16, background: "var(--qu-border)", margin: "0 4px" }} />
      {item("Copy (Ctrl+C)", Copy, canEditSelection, onCopy)}
      {item("Paste (Ctrl+V)", ClipboardPaste, hasClipboard, onPaste)}
      {item("Duplicate (Ctrl+D)", Layers, canEditSelection, onDuplicate)}
      {item("Delete (Del)", Trash2, canEditSelection, onDelete)}
      <span style={{ marginLeft: "auto", fontSize: 10, color: muted }}>
        Shift-click to multi-select · F2 rename · Ctrl+↑/↓ reorder
      </span>
    </div>
  );
};

const OutlineRow: React.FC<{
  node: DesignNode;
  depth: number;
  selected: boolean;
  isInsertPoint: boolean;
  hint: DropWhere | null;
  renaming: boolean;
  onRename: (name: string | null) => void;
  onStartRename: () => void;
  onSelect: (additive: boolean) => void;
  dragProps: Record<string, unknown>;
  dropProps: Record<string, unknown>;
}> = ({ node, depth, selected, isInsertPoint, hint, renaming, onRename, onStartRename, onSelect, dragProps, dropProps }) => {
  const Icon = KIND_ICON[node.kind];
  const bound = eventsFor(node.kind).find((event) => node.events[event]);
  return (
    <div style={{ borderTop: hint === "before" ? `2px solid ${ACCENT}` : "2px solid transparent", borderBottom: hint === "after" ? `2px solid ${ACCENT}` : "2px solid transparent" }}>
      <div
        role="treeitem"
        aria-selected={selected}
        tabIndex={-1}
        onClick={(event) => {
          event.stopPropagation();
          onSelect(event.shiftKey);
        }}
        onDoubleClick={(event) => {
          event.stopPropagation();
          onStartRename();
        }}
        {...dragProps}
        {...dropProps}
        style={{
          display: "flex",
          alignItems: "center",
          gap: 5,
          padding: "3px 6px",
          paddingLeft: 6 + depth * 12,
          borderRadius: 4,
          fontSize: 11,
          cursor: "pointer",
          background: selected ? ACCENT_SOLID : hint === "into" ? "var(--qu-accent-soft)" : "transparent",
          color: selected ? "#fff" : undefined,
          outline: isInsertPoint && !selected ? `1px dashed ${ACCENT}` : undefined,
        }}
      >
        <Icon size={11} />
        {renaming ? (
          <input
            autoFocus
            defaultValue={node.varName}
            onBlur={(event) => onRename(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") onRename((event.target as HTMLInputElement).value);
              if (event.key === "Escape") onRename(null);
              event.stopPropagation();
            }}
            style={{ fontSize: 11, width: "100%", border: `1px solid ${ACCENT}`, borderRadius: 3, padding: "0 3px" }}
          />
        ) : (
          <>
            <strong style={{ fontWeight: 600 }}>{node.varName}</strong>
            <span style={{ color: selected ? "rgba(255,255,255,0.75)" : muted }}>{node.kind}</span>
            {bound && (
              <span title={`${bound} → ${node.events[bound]}`} style={{ marginLeft: "auto", fontSize: 9, color: selected ? "#fff" : ACCENT }}>
                ƒ
              </span>
            )}
          </>
        )}
      </div>
    </div>
  );
};

const CodeStrip: React.FC<{ code: string; onInsertCode?: (code: string) => void }> = ({ code, onInsertCode }) => (
  <div style={{ borderTop: border, display: "flex", flexDirection: "column", maxHeight: 170 }}>
    <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "5px 10px" }}>
      <h3 style={{ fontSize: 11, textTransform: "uppercase", color: muted, letterSpacing: 0.4 }}>form.design.qu</h3>
      <span style={{ marginLeft: "auto" }} />
      <button title="Copy form.design.qu" onClick={() => void navigator.clipboard?.writeText(code)} style={{ border: "none", background: "transparent", cursor: "pointer" }}>
        <Copy size={13} />
      </button>
      {onInsertCode && (
        <button title="Sync to form.design.qu, seed any new handler stubs into form.code.qu" onClick={() => onInsertCode(code)} style={{ border: "none", background: "transparent", cursor: "pointer", color: ACCENT }}>
          <FilePlus2 size={13} />
        </button>
      )}
    </div>
    <pre style={{ margin: 0, padding: "0 10px 10px", fontSize: 11, lineHeight: 1.5, overflow: "auto", background: "var(--qu-code-bg)", whiteSpace: "pre" }}>{code}</pre>
  </div>
);

/** Property fields for the selection, plus its event bindings. Reads
 *  `propsFor(kind)` rather than a hand-written chain of `kind === "..."`
 *  guards, so it can only ever show props `gui.rs`'s `validate()` accepts
 *  for that kind -- see `guiDesign.ts`'s own comment on why the schema
 *  mirrors the engine rather than inventing a separate prop set.
 *
 *  Multi-select shows the INTERSECTION of the selected kinds' props and
 *  writes every edit to all of them, which is what VB6's own Properties
 *  window does; showing the union would offer, say, `min` for a button in
 *  a mixed selection and generate a node the engine refuses. */
const Inspector: React.FC<{
  nodes: DesignNode[];
  selected: DesignNode[];
  primary: DesignNode;
  byCategory: boolean;
  commit: (next: (prev: DesignNode[]) => DesignNode[]) => void;
  onStartRename: () => void;
}> = ({ nodes, selected, primary, byCategory, commit, onStartRename }) => {
  const multi = selected.length > 1;
  const specs = useMemo(() => {
    const lists = selected.map((node) => propsFor(node.kind));
    const shared = lists[0]?.filter((spec) => lists.every((list) => list.some((other) => other.key === spec.key))) ?? [];
    return byCategory ? shared : [...shared].sort((a, b) => a.label.localeCompare(b.label));
  }, [selected, byCategory]);

  const patch = (p: DesignProps) => commit((prev) => selected.reduce((acc, node) => updateProps(acc, node.id, p), prev));
  const events = multi ? [] : eventsFor(primary.kind);
  // gui.rs enforces min < max and min <= value <= max, and rejects the
  // whole node otherwise. Surfaced here rather than only at Run so the
  // error arrives while you are still looking at the field that caused it.
  const rangeError =
    !multi && (primary.kind === "slider" || primary.kind === "number")
      ? (() => {
          const min = primary.props.min ?? 0;
          const max = primary.props.max ?? 100;
          const value = typeof primary.props.value === "number" ? primary.props.value : min;
          if (min >= max) return "min must be less than max";
          if (value < min || value > max) return "value must sit between min and max";
          return null;
        })()
      : null;

  const rows = (list: PropSpec[]) =>
    list.map((spec) => (
      <PropertyRow key={String(spec.key)} spec={spec} node={primary} multi={multi} nodes={nodes} commit={commit} selected={selected} patch={patch} />
    ));

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
      <div style={{ display: "flex", alignItems: "baseline", gap: 6, paddingBottom: 6, borderBottom: border }}>
        <strong style={{ fontSize: 12 }}>{multi ? `${selected.length} widgets` : primary.varName}</strong>
        <span style={{ fontSize: 11, color: muted }}>{multi ? selected.map((n) => n.kind).join(", ") : primary.kind}</span>
        {!multi && (
          <button onClick={onStartRename} title="Rename (F2)" style={{ marginLeft: "auto", border, borderRadius: 4, background: "transparent", fontSize: 10, padding: "1px 5px", cursor: "pointer" }}>
            Rename
          </button>
        )}
      </div>

      {rangeError && (
        <p role="alert" style={{ fontSize: 10, color: "var(--qu-danger)", margin: 0 }}>
          {rangeError} — the engine will refuse this widget.
        </p>
      )}

      {byCategory
        ? PROP_CATEGORIES.map((category) => {
            const list = specs.filter((spec) => spec.category === category);
            if (!list.length) return null;
            return (
              <section key={category}>
                <h4 style={{ fontSize: 10, textTransform: "uppercase", color: muted, margin: "4px 0", letterSpacing: 0.3 }}>{category}</h4>
                <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>{rows(list)}</div>
              </section>
            );
          })
        : <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>{rows(specs)}</div>}

      {events.length > 0 && (
        <section>
          <h4 style={{ fontSize: 10, textTransform: "uppercase", color: muted, margin: "6px 0", letterSpacing: 0.3 }}>Events</h4>
          {events.map((event) => (
            <div key={event} style={{ display: "flex", gap: 6, alignItems: "center", marginBottom: 4 }}>
              <span style={{ fontSize: 11, width: 46, color: muted }}>{event}</span>
              <input
                value={primary.events[event] ?? ""}
                placeholder={defaultHandlerName(primary, event)}
                onChange={(e) => commit((prev) => setEvent(prev, primary.id, event, e.target.value.trim() || null))}
                style={{ flex: 1, minWidth: 0, padding: "3px 5px", border, borderRadius: 4, fontSize: 11 }}
              />
            </div>
          ))}
          <p style={{ fontSize: 10, color: muted, margin: 0 }}>
            Double-click the widget to generate this handler and its stub.
          </p>
        </section>
      )}
    </div>
  );
};

/** One typed editor, chosen by `PropSpec.editor`. The point of driving
 *  this from the schema is that an editor can only produce values the
 *  engine's `validate()` accepts for that key -- a bare `<input>` for
 *  `layout` could type `flex` and generate a node `gui.rs` rejects. */
const PropertyRow: React.FC<{
  spec: PropSpec;
  node: DesignNode;
  nodes: DesignNode[];
  selected: DesignNode[];
  multi: boolean;
  commit: (next: (prev: DesignNode[]) => DesignNode[]) => void;
  patch: (p: DesignProps) => void;
}> = ({ spec, node, selected, multi, commit, patch }) => {
  const value = node.props[spec.key];
  const input: React.CSSProperties = { padding: "3px 5px", border, borderRadius: 4, fontSize: 11, width: "100%", minWidth: 0 };

  const control = (() => {
    switch (spec.editor) {
      case "bool":
        return (
          <input
            type="checkbox"
            checked={value === true}
            onChange={(e) => patch({ [spec.key]: e.target.checked } as DesignProps)}
          />
        );
      case "number":
        return (
          <input
            type="number"
            value={typeof value === "number" ? value : ""}
            onChange={(e) => patch({ [spec.key]: e.target.value === "" ? undefined : Number(e.target.value) } as DesignProps)}
            style={input}
          />
        );
      case "enum": {
        // `value` on a select has no fixed choice list -- it is whatever
        // `options` currently holds, so the dropdown is built from the
        // sibling prop rather than from the schema.
        const choices = spec.choices ?? (spec.key === "value" ? (node.props.options ?? []) : []);
        return (
          <select
            value={typeof value === "string" ? value : ""}
            onChange={(e) => patch({ [spec.key]: e.target.value } as DesignProps)}
            style={input}
          >
            {choices.map((choice) => (
              <option key={choice} value={choice}>{choice}</option>
            ))}
          </select>
        );
      }
      case "strings":
        return (
          <textarea
            rows={Math.min(6, Math.max(2, (node.props.options?.length ?? 1) + 1))}
            value={(node.props.options ?? []).join("\n")}
            // Routed through `setOptions`, not `patch`: dropping the
            // option a select currently holds must move `value` too or the
            // pair stops validating (see `setOptions`).
            onChange={(e) =>
              commit((prev) =>
                selected.reduce((acc, target) => setOptions(acc, target.id, e.target.value.split("\n").map((line) => line.trim()).filter(Boolean)), prev),
              )
            }
            style={{ ...input, fontFamily: "inherit", resize: "vertical" }}
          />
        );
      case "expr":
        return (
          <input
            value={typeof value === "string" ? value : ""}
            placeholder="Qu expression"
            onChange={(e) => patch({ [spec.key]: e.target.value } as DesignProps)}
            style={{ ...input, fontFamily: "monospace" }}
          />
        );
      default:
        return (
          <input
            value={typeof value === "string" ? value : ""}
            onChange={(e) => patch({ [spec.key]: e.target.value } as DesignProps)}
            style={input}
          />
        );
    }
  })();

  const mixed = multi && selected.some((other) => other.props[spec.key] !== value);
  return (
    <label style={{ display: "grid", gridTemplateColumns: "84px 1fr", alignItems: "center", gap: 6, fontSize: 11 }}>
      <span style={{ color: muted }} title={spec.hint}>
        {spec.label}
        {mixed && <em style={{ fontStyle: "normal", color: ACCENT }}> *</em>}
      </span>
      {control}
    </label>
  );
};

export default GuiDesigner;
