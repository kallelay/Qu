import { quote, forCode } from "./useFigureTools";

/** The design-time model behind `GuiDesigner.tsx`, and the pure function
 * that turns it into real Qu source.
 *
 * This mirrors `engine/crates/qu-interp/src/gui.rs`'s `validate()` kind/
 * prop table and its `.on()` event match arm on purpose: nothing this
 * module can produce should ever be able to fail the engine's own
 * server-side validation. If `gui.rs` grows a new widget kind or prop,
 * update `WidgetKind`/`DesignProps`/`defaultProps`/`EVENTS_BY_KIND` below
 * to match -- treat `gui.rs` as the source of truth, this file as its
 * mirror.
 *
 * One-way generation only (see `docs/design/qu-gui-designer-spec.md` §4):
 * this module never reads a `.qu` file back into a tree. It only ever
 * produces text to hand to an editor, the same relationship
 * `useFigureTools.ts`'s `shapeCode` has with the script it feeds.
 */

/** Palette entries. `frame` is the implicit root and is never placed from
 *  the palette -- one design has exactly one. */
export type WidgetKind =
  | "panel"
  | "label"
  | "button"
  | "slider"
  | "number"
  | "text"
  | "checkbox"
  | "select"
  | "plot";

export type NodeKind = "frame" | WidgetKind;

export const CONTAINER_KINDS: NodeKind[] = ["frame", "panel"];

/** Events `gui.rs`'s `.on()` accepts, keyed by kind -- exactly the match
 *  arm in `gui_call`'s `"on"` case. */
const EVENTS_BY_KIND: Partial<Record<NodeKind, Array<"click" | "change" | "close">>> = {
  button: ["click"],
  slider: ["change"],
  number: ["change"],
  checkbox: ["change"],
  text: ["change"],
  select: ["change"],
  frame: ["close"],
};

export function eventsFor(kind: NodeKind): Array<"click" | "change" | "close"> {
  return EVENTS_BY_KIND[kind] ?? [];
}

export interface DesignProps {
  title?: string;
  text?: string;
  xlabel?: string;
  ylabel?: string;
  layout?: "row" | "column" | "grid";
  visible?: boolean;
  disabled?: boolean;
  equal_aspect?: boolean;
  min?: number;
  max?: number;
  /** Numeric for slider/number, boolean for checkbox, string for text and
   *  select. For a select `gui.rs` additionally requires this be one OF
   *  `options` -- see `setOptions`, which keeps the pair consistent. */
  value?: number | boolean | string;
  /** Select only: the dropdown's choices. */
  options?: string[];
  /** Plot only. Raw Qu expressions (e.g. `x`, `sin(x)`), not literals --
   *  a plot's data comes from script-level variables the designer cannot
   *  see or author (§4 of the spec). Inserted verbatim, unquoted. */
  x?: string;
  y?: string;
}

export interface DesignEvents {
  click?: string;
  change?: string;
  close?: string;
}

export interface DesignNode {
  id: string;
  kind: NodeKind;
  /** The Qu variable name this node is assigned to, e.g. `button1`. User-
   *  editable; renaming updates every reference (see `renameNode`). */
  varName: string;
  parentId: string | null;
  props: DesignProps;
  events: DesignEvents;
}

export const ROOT_ID = "root";

let idCounter = 0;
function freshId(): string {
  idCounter += 1;
  return `n${idCounter}${Math.random().toString(36).slice(2, 6)}`;
}

export function createRoot(title = "My Qu app"): DesignNode {
  // `layout` must be set, not left implicit: both `GuiDesigner`'s canvas and
  // `GuiPanel`'s runtime render already treat an absent `layout` as
  // "column" (flexDirection defaults to column unless "row"), but the
  // inspector's Layout dropdown has no blank option -- an unset `layout`
  // made it display "row" while the model actually held undefined, so
  // picking "row" looked like a no-op until you chose something else first.
  return { id: ROOT_ID, kind: "frame", varName: "t", parentId: null, props: { title, layout: "column" }, events: {} };
}

/** Sensible starting props for a freshly-placed widget, matching what a
 *  person would type by hand for a first, valid, runnable instance --
 *  `gui.rs`'s `validate()` requires `min < max` and `min <= value <= max`
 *  for slider/number, so a blank/zeroed default would be rejected. */
function defaultProps(kind: NodeKind): DesignProps {
  switch (kind) {
    case "panel":
      return { layout: "row" };
    case "label":
      return { text: "Label" };
    case "button":
      return { text: "Button" };
    case "slider":
    case "number":
      return { text: kind === "slider" ? "Slider" : "Number", min: 0, max: 100, value: 0 };
    case "checkbox":
      return { text: "Checkbox", value: false };
    case "text":
      return { text: "Text", value: "" };
    case "select":
      // `value` must already be one of `options` or `gui.rs` refuses the
      // node outright -- a blank default would be dead on arrival.
      return { text: "Select", options: ["Option 1", "Option 2"], value: "Option 1" };
    case "plot":
      return { title: "Plot", xlabel: "x", ylabel: "y", x: "x", y: "y" };
    default:
      return {};
  }
}

function countOfKind(nodes: DesignNode[], kind: NodeKind): number {
  return nodes.filter((n) => n.kind === kind).length;
}

export function nextVarName(nodes: DesignNode[], kind: NodeKind): string {
  let n = countOfKind(nodes, kind) + 1;
  const taken = new Set(nodes.map((node) => node.varName));
  while (taken.has(`${kind}${n}`)) n += 1;
  return `${kind}${n}`;
}

/** Append a new widget as `parentId`'s last child. `parentId` must name an
 *  existing container node (`frame` or `panel`) -- mirrors `gui.rs`'s own
 *  "Only frames and panels can contain widgets" check, checked here too so
 *  the UI can refuse the drop before it ever reaches generated code. */
export function addNode(nodes: DesignNode[], parentId: string, kind: WidgetKind): DesignNode[] {
  const parent = nodes.find((n) => n.id === parentId);
  if (!parent || !CONTAINER_KINDS.includes(parent.kind)) return nodes;
  const node: DesignNode = {
    id: freshId(),
    kind,
    varName: nextVarName(nodes, kind),
    parentId,
    props: defaultProps(kind),
    events: {},
  };
  return nodes.concat(node);
}

function descendantIds(nodes: DesignNode[], id: string): Set<string> {
  const out = new Set<string>();
  const stack = [id];
  while (stack.length) {
    const cur = stack.pop()!;
    for (const n of nodes) {
      if (n.parentId === cur && !out.has(n.id)) {
        out.add(n.id);
        stack.push(n.id);
      }
    }
  }
  return out;
}

/** Remove a node and everything nested inside it. The root can't be
 *  removed -- a design always has exactly one `Frame`. */
export function removeNode(nodes: DesignNode[], id: string): DesignNode[] {
  if (id === ROOT_ID) return nodes;
  const gone = descendantIds(nodes, id);
  gone.add(id);
  return nodes.filter((n) => !gone.has(n.id));
}

export function updateProps(nodes: DesignNode[], id: string, patch: DesignProps): DesignNode[] {
  return nodes.map((n) => (n.id === id ? { ...n, props: { ...n.props, ...patch } } : n));
}

/** Replace a select's option list, re-pointing `value` if the old choice
 *  is gone. `gui.rs` validates `options` and `value` as a PAIR (a value
 *  outside the list is refused), so editing the list alone would generate
 *  a script the engine rejects the moment the old value stops existing --
 *  the same atomicity trap `set(x=..., y=...)` has for plots. */
export function setOptions(nodes: DesignNode[], id: string, options: string[]): DesignNode[] {
  return nodes.map((n) => {
    if (n.id !== id || n.kind !== "select") return n;
    const current = n.props.value;
    const keep = typeof current === "string" && options.includes(current);
    return { ...n, props: { ...n.props, options, value: keep ? current : options[0] } };
  });
}

/** VB6's Properties window groups by category before it sorts; these are
 *  the four headings it actually uses. `Data` means "bound to something
 *  your script owns" (a plot's arrays, a select's choices), which is the
 *  distinction that matters here -- those are the props the designer
 *  cannot author, only reference. */
export type PropCategory = "Appearance" | "Behavior" | "Layout" | "Data";

export const PROP_CATEGORIES: PropCategory[] = ["Appearance", "Behavior", "Layout", "Data"];

export interface PropSpec {
  key: keyof DesignProps;
  label: string;
  category: PropCategory;
  /** Which inline editor the inspector renders. `expr` is a raw Qu
   *  expression inserted verbatim; `strings` is an editable string list. */
  editor: "string" | "number" | "bool" | "enum" | "expr" | "strings";
  choices?: readonly string[];
  hint?: string;
}

/** Every prop `gui.rs`'s `validate()` accepts, per kind, with the editor
 *  that can only produce values it accepts. Kept as data rather than JSX
 *  branches so the inspector, the "reset to default" action and any future
 *  consumer all read one table -- the previous hand-written chain of
 *  `kind === "..."` guards in `GuiDesigner.tsx` had already drifted into
 *  showing `title` for the frame but not offering `visible`/`disabled` for
 *  anything at all. */
const COMMON_BEHAVIOR: PropSpec[] = [
  { key: "visible", label: "Visible", category: "Behavior", editor: "bool" },
  { key: "disabled", label: "Disabled", category: "Behavior", editor: "bool" },
];
const TEXT_LABEL: PropSpec = { key: "text", label: "Text", category: "Appearance", editor: "string" };
const RANGE: PropSpec[] = [
  { key: "min", label: "Min", category: "Behavior", editor: "number" },
  { key: "max", label: "Max", category: "Behavior", editor: "number" },
  { key: "value", label: "Value", category: "Behavior", editor: "number", hint: "Must satisfy min <= value <= max." },
];
const LAYOUT: PropSpec = {
  key: "layout",
  label: "Layout",
  category: "Layout",
  editor: "enum",
  choices: ["row", "column", "grid"],
  hint: "The runtime has no freeform x/y placement -- only these three.",
};

const PROPS_BY_KIND: Record<NodeKind, PropSpec[]> = {
  // No `visible` for the root: `gui.rs` forces it false inside `Frame(...)`
  // (overwriting any kwarg) and `.show()` sets it true, so an inspector
  // control for it would be a switch wired to nothing.
  frame: [
    { key: "title", label: "Title", category: "Appearance", editor: "string" },
    LAYOUT,
    { key: "disabled", label: "Disabled", category: "Behavior", editor: "bool" },
  ],
  panel: [LAYOUT, ...COMMON_BEHAVIOR],
  label: [TEXT_LABEL, ...COMMON_BEHAVIOR],
  button: [TEXT_LABEL, ...COMMON_BEHAVIOR],
  slider: [TEXT_LABEL, ...RANGE, ...COMMON_BEHAVIOR],
  number: [TEXT_LABEL, ...RANGE, ...COMMON_BEHAVIOR],
  text: [TEXT_LABEL, { key: "value", label: "Value", category: "Behavior", editor: "string" }, ...COMMON_BEHAVIOR],
  checkbox: [TEXT_LABEL, { key: "value", label: "Checked", category: "Behavior", editor: "bool" }, ...COMMON_BEHAVIOR],
  select: [
    TEXT_LABEL,
    { key: "options", label: "Options", category: "Data", editor: "strings", hint: "One choice per line." },
    { key: "value", label: "Selected", category: "Behavior", editor: "enum" },
    ...COMMON_BEHAVIOR,
  ],
  plot: [
    { key: "title", label: "Title", category: "Appearance", editor: "string" },
    { key: "xlabel", label: "X label", category: "Appearance", editor: "string" },
    { key: "ylabel", label: "Y label", category: "Appearance", editor: "string" },
    { key: "x", label: "X data", category: "Data", editor: "expr", hint: "A Qu expression your script defines, e.g. x." },
    { key: "y", label: "Y data", category: "Data", editor: "expr", hint: "A Qu expression, e.g. sin(x)." },
    { key: "equal_aspect", label: "Equal aspect", category: "Layout", editor: "bool" },
    ...COMMON_BEHAVIOR,
  ],
};

export function propsFor(kind: NodeKind): PropSpec[] {
  return PROPS_BY_KIND[kind] ?? [];
}

/** Bind or clear an event handler. `handlerName` of `null` clears it --
 *  mirrors "no `.on()` call" rather than an empty string, which `gui.rs`
 *  would reject as an undefined function name. */
export function setEvent(
  nodes: DesignNode[],
  id: string,
  event: "click" | "change" | "close",
  handlerName: string | null,
): DesignNode[] {
  return nodes.map((n) => {
    if (n.id !== id) return n;
    const events = { ...n.events };
    if (handlerName) events[event] = handlerName;
    else delete events[event];
    return { ...n, events };
  });
}

/** Rename a node's Qu variable. Refuses a collision with another node's
 *  current name -- generated code with two `button1`s would shadow one of
 *  them silently, which is worse than refusing the rename outright. */
export function renameNode(nodes: DesignNode[], id: string, varName: string): DesignNode[] {
  const trimmed = varName.trim();
  if (!/^[A-Za-z_][A-Za-z0-9_]*$/.test(trimmed)) return nodes;
  if (nodes.some((n) => n.id !== id && n.varName === trimmed)) return nodes;
  return nodes.map((n) => (n.id === id ? { ...n, varName: trimmed } : n));
}

export function children(nodes: DesignNode[], parentId: string | null): DesignNode[] {
  return nodes.filter((n) => n.parentId === parentId);
}

/** Array order IS sibling order (both `children` and `designTreeToQu`'s
 *  walk read it), so reordering and reparenting are both splices. Depth-
 *  first emission means a node never has to sit after its parent in the
 *  array for the generated code to assign the parent first -- only sibling
 *  order is carried here. */
function spliceUnder(nodes: DesignNode[], moving: DesignNode[], beforeId: string | null): DesignNode[] {
  const movingIds = new Set(moving.map((n) => n.id));
  const rest = nodes.filter((n) => !movingIds.has(n.id));
  const at = beforeId ? rest.findIndex((n) => n.id === beforeId) : -1;
  if (at < 0) return [...rest, ...moving];
  return [...rest.slice(0, at), ...moving, ...rest.slice(at)];
}

/** Move `id` to be a child of `parentId`, inserted before `beforeId` (or
 *  appended when null). Refuses the two moves that would corrupt the tree:
 *  dropping the root, and dropping a node into its own descendant (which
 *  would detach that whole subtree from the root and silently vanish it
 *  from generated code, since codegen only walks down from ROOT). */
export function moveNode(nodes: DesignNode[], id: string, parentId: string, beforeId: string | null = null): DesignNode[] {
  if (id === ROOT_ID || id === parentId) return nodes;
  const parent = nodes.find((n) => n.id === parentId);
  const node = nodes.find((n) => n.id === id);
  if (!parent || !node || !CONTAINER_KINDS.includes(parent.kind)) return nodes;
  const inside = descendantIds(nodes, id);
  if (inside.has(parentId)) return nodes;
  if (beforeId && (beforeId === id || inside.has(beforeId))) return nodes;
  const subtree = nodes.filter((n) => n.id !== id && inside.has(n.id));
  return spliceUnder(nodes, [{ ...node, parentId }, ...subtree], beforeId);
}

/** Deep-copy a subtree next to the original, every node given a fresh id
 *  and a fresh `{kind}{n}` name. Event bindings are carried over BY NAME
 *  rather than renamed: two buttons sharing one handler is a deliberate,
 *  supported pattern (`designTreeToQu` already de-duplicates the stub), and
 *  a copy that silently pointed at a handler nobody wrote would generate a
 *  script `gui.rs` refuses with "Define function ... before registering". */
export function duplicateNode(nodes: DesignNode[], id: string): { nodes: DesignNode[]; newId: string | null } {
  const node = nodes.find((n) => n.id === id);
  if (!node || id === ROOT_ID) return { nodes, newId: null };
  const inside = descendantIds(nodes, id);
  const originals = nodes.filter((n) => n.id === id || inside.has(n.id));
  const idMap = new Map<string, string>();
  let naming = nodes;
  const copies: DesignNode[] = [];
  for (const original of originals) {
    const copy: DesignNode = {
      ...original,
      id: freshId(),
      varName: nextVarName(naming, original.kind),
      props: { ...original.props, ...(original.props.options ? { options: [...original.props.options] } : {}) },
      events: { ...original.events },
    };
    idMap.set(original.id, copy.id);
    // Thread the growing list through `nextVarName` or every copy in one
    // multi-node subtree would be handed the same free name.
    naming = naming.concat(copy);
    copies.push(copy);
  }
  const rehomed = copies.map((copy, index) => {
    const original = originals[index];
    const mapped = original.parentId ? idMap.get(original.parentId) : null;
    return { ...copy, parentId: mapped ?? original.parentId };
  });
  const at = nodes.findIndex((n) => n.id === id);
  const out = [...nodes.slice(0, at + 1), ...rehomed, ...nodes.slice(at + 1)];
  return { nodes: out, newId: idMap.get(id) ?? null };
}

/** Default handler name for a freshly-bound event -- `{var}_{event}`,
 *  e.g. `button1_click`. Matches VB6/App Designer's own "name the callback
 *  after what it's on" convention. */
export function defaultHandlerName(node: DesignNode, event: string): string {
  return `${node.varName}_${event}`;
}

/** Property emission order per kind, chosen for readability (what a
 *  person would type first) rather than any structural requirement --
 *  `gui.rs` does not care about argument order. */
const PROP_ORDER: Array<keyof DesignProps> = [
  "title",
  "layout",
  "text",
  "xlabel",
  "ylabel",
  "x",
  "y",
  "options",
  "min",
  "max",
  "value",
  "equal_aspect",
  "visible",
  "disabled",
];

function encodePropValue(key: keyof DesignProps, value: unknown): string | null {
  if (value === undefined || value === null) return null;
  if (key === "x" || key === "y") {
    // Raw Qu expressions, e.g. `x` or `sin(x)` -- not a designer-owned
    // literal, so no quoting/escaping applies (§4 of the spec).
    const expr = String(value).trim();
    return expr.length ? expr : null;
  }
  if (Array.isArray(value)) {
    // `options` -- a real Qu list literal of quoted strings, so it needs
    // the same brace-doubling escape any other designer-owned string does.
    return value.length ? `[${value.map((entry) => quote(String(entry))).join(", ")}]` : null;
  }
  if (typeof value === "boolean") return value ? "true" : "false";
  if (typeof value === "number") return forCode(value);
  if (typeof value === "string") return quote(value);
  return null;
}

/** The one real output of this module: a design tree -> a runnable Qu
 *  script, in the same shape a person would hand-write (see the spec's
 *  worked example). Function declarations always precede `.on(...)`
 *  calls, matching `gui.rs`'s own requirement ("Define function `{name}`
 *  before registering its handler") -- getting this order wrong would
 *  generate a script the engine refuses to run.
 */
export function designTreeToQu(nodes: DesignNode[]): string[] {
  const root = nodes.find((n) => n.id === ROOT_ID);
  if (!root) return [];
  const lines: string[] = [];
  const byParent = new Map<string | null, DesignNode[]>();
  for (const n of nodes) {
    const list = byParent.get(n.parentId) ?? [];
    list.push(n);
    byParent.set(n.parentId, list);
  }

  lines.push(`${root.varName} = Frame(${quote(root.props.title ?? "My Qu app")})`);
  // The root's OTHER props need their own `.set(...)`: `Frame(...)` takes
  // the title positionally and `gui.rs` overwrites both `title` and
  // `visible` after applying kwargs, so anything else passed there would
  // be silently dropped. `visible` is deliberately never emitted -- the
  // trailing `.show()` owns it.
  const rootExtras: string[] = [];
  for (const key of PROP_ORDER) {
    if (key === "title" || key === "visible") continue;
    const encoded = encodePropValue(key, root.props[key]);
    if (encoded !== null) rootExtras.push(`${key}=${encoded}`);
  }
  if (rootExtras.length) lines.push(`${root.varName}.set(${rootExtras.join(", ")})`);

  const visit = (parentId: string) => {
    for (const node of byParent.get(parentId) ?? []) {
      const parentVar = nodes.find((n) => n.id === parentId)!.varName;
      const args = [quote(node.kind)];
      for (const key of PROP_ORDER) {
        const encoded = encodePropValue(key, node.props[key]);
        if (encoded !== null) args.push(`${key}=${encoded}`);
      }
      lines.push(`${node.varName} = ${parentVar}.add(${args.join(", ")})`);
      visit(node.id);
    }
  };
  visit(ROOT_ID);

  // `.on(...)` bindings only -- NOT the handler function bodies. Those
  // live in the user's own `form.code.qu` now (§ Design/Code/Run split,
  // 2026-09-16, Ahmed's direct instruction): this function's whole output
  // is machine-owned and safe to regenerate wholesale on every design
  // change, the same way VB6/WinForms' `Designer.cs` is -- and a
  // function BODY is exactly the kind of thing a design-tool regeneration
  // must never touch, because it's where the user's own logic lives.
  // `requiredHandlerNames` below walks the same tree to say which handler
  // NAMES this binding list requires; `reconcileCodeQu` uses that list to
  // seed stubs into `form.code.qu` without ever overwriting an existing
  // one.
  const bindings: string[] = [];
  const walk = (id: string) => {
    const node = nodes.find((n) => n.id === id);
    if (node) {
      for (const [event, handler] of Object.entries(node.events)) {
        if (!handler) continue;
        bindings.push(`${node.varName}.on(${quote(event)}, ${quote(handler)})`);
      }
    }
    for (const child of byParent.get(id) ?? []) walk(child.id);
  };
  walk(ROOT_ID);

  if (bindings.length) {
    lines.push("");
    lines.push(...bindings);
  }
  lines.push("");
  lines.push(`${root.varName}.show()`);
  return lines;
}

/** The distinct handler names `designTreeToQu`'s bindings will reference,
 *  in the order first seen (stable, so `reconcileCodeQu` appends new
 *  stubs in a predictable order rather than shuffling on every design
 *  edit). Duplicates happen when the same handler name is reused across
 *  widgets on purpose (e.g. two sliders sharing one recompute function)
 *  and are only counted once.
 */
export function requiredHandlerNames(nodes: DesignNode[]): string[] {
  const byParent = new Map<string | null, DesignNode[]>();
  for (const n of nodes) {
    const list = byParent.get(n.parentId) ?? [];
    list.push(n);
    byParent.set(n.parentId, list);
  }
  const seen = new Set<string>();
  const names: string[] = [];
  const walk = (id: string) => {
    const node = nodes.find((n) => n.id === id);
    if (node) {
      for (const handler of Object.values(node.events)) {
        if (handler && !seen.has(handler)) {
          seen.add(handler);
          names.push(handler);
        }
      }
    }
    for (const child of byParent.get(id) ?? []) walk(child.id);
  };
  walk(ROOT_ID);
  return names;
}

/** Adds an empty stub (`function NAME(event)\n\nend function`) to
 *  `existingCode` for every name in `requiredNames` that isn't ALREADY
 *  defined there (matched by a `function NAME(` line, so a name the user
 *  already wrote a real body for is never touched). A name no longer
 *  required (the widget that bound it was deleted or rebound) is left in
 *  place rather than deleted -- this is the user's own file, and
 *  silently removing code they wrote because a design edit stopped
 *  referencing it would be exactly the kind of surprise a design tool
 *  regenerating a SEPARATE, machine-owned file must never cause here.
 */
export function reconcileCodeQu(existingCode: string, requiredNames: string[]): string {
  const definedNames = new Set<string>();
  const defRe = /^\s*function\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(/gm;
  let m: RegExpExecArray | null;
  while ((m = defRe.exec(existingCode)) !== null) definedNames.add(m[1]);

  const missing = requiredNames.filter((n) => !definedNames.has(n));
  if (missing.length === 0) return existingCode;

  const stubs = missing.map((n) => `function ${n}(event)\n\nend function`);
  const trimmed = existingCode.trimEnd();
  const prefix = trimmed.length > 0 ? trimmed + "\n\n" : "";
  return prefix + stubs.join("\n\n") + "\n";
}

/** `form.code.qu` + `form.design.qu` -> the script that actually runs
 *  (`form.gen.qu`). Code first, design second: `gui.rs` requires a
 *  handler function to be DEFINED (i.e. already executed as a `function`
 *  statement) before the `.on(...)` call that registers it runs, and
 *  every function lives in `codeQu` while every `.on(...)` binding lives
 *  in `designQu` -- putting all of one before all of the other satisfies
 *  that regardless of which specific widget a handler belongs to.
 */
export function mergeGenScript(codeQu: string, designQu: string): string {
  const parts = [codeQu.trim(), designQu.trim()].filter((s) => s.length > 0);
  return parts.join("\n\n") + "\n";
}
