import { describe, expect, it } from "vitest";
import {
  ROOT_ID,
  addNode,
  createRoot,
  designTreeToQu,
  duplicateNode,
  mergeGenScript,
  moveNode,
  propsFor,
  reconcileCodeQu,
  removeNode,
  requiredHandlerNames,
  setEvent,
  setOptions,
  updateProps,
  children,
} from "./guiDesign";
import type { DesignNode } from "./guiDesign";

/** These cover the invariants that, if broken, generate a script
 *  `engine/crates/qu-interp/src/gui.rs` REFUSES at run time -- the failure
 *  mode that matters, because the designer's whole promise is that it
 *  cannot emit something the engine rejects. Pure-function level: the
 *  React surface is exercised by hand, this is the part worth pinning. */

const build = () => {
  let nodes: DesignNode[] = [createRoot("Signal laboratory")];
  nodes = addNode(nodes, ROOT_ID, "panel");
  const panel = nodes[nodes.length - 1];
  nodes = addNode(nodes, panel.id, "slider");
  nodes = addNode(nodes, panel.id, "label");
  return { nodes, panelId: panel.id };
};

describe("design tree mutation", () => {
  it("reparents a subtree and keeps sibling order as array order", () => {
    const { nodes, panelId } = build();
    const label = nodes.find((n) => n.kind === "label")!;
    const slider = nodes.find((n) => n.kind === "slider")!;
    // Move the label out of the panel, up to the root, before the panel.
    const moved = moveNode(nodes, label.id, ROOT_ID, panelId);
    expect(children(moved, ROOT_ID).map((n) => n.kind)).toEqual(["label", "panel"]);
    expect(children(moved, panelId).map((n) => n.kind)).toEqual(["slider"]);
    // Reordering within one parent is the same operation.
    const back = moveNode(moved, label.id, panelId, slider.id);
    expect(children(back, panelId).map((n) => n.kind)).toEqual(["label", "slider"]);
  });

  it("refuses the moves that would detach a subtree from the root", () => {
    const { nodes, panelId } = build();
    const slider = nodes.find((n) => n.kind === "slider")!;
    // A container into its own descendant: codegen only walks DOWN from
    // ROOT, so this would delete the subtree from the output silently.
    expect(moveNode(nodes, panelId, panelId)).toBe(nodes);
    expect(moveNode(nodes, ROOT_ID, panelId)).toBe(nodes);
    // Non-containers can't take children -- gui.rs's own rule.
    expect(moveNode(nodes, panelId, slider.id)).toBe(nodes);
  });

  it("duplicates a subtree with fresh, non-colliding names", () => {
    const { nodes, panelId } = build();
    const { nodes: out, newId } = duplicateNode(nodes, panelId);
    expect(newId).toBeTruthy();
    const names = out.map((n) => n.varName);
    expect(new Set(names).size).toBe(names.length);
    // The copy is a real subtree, not a shallow clone sharing children.
    expect(children(out, newId!).map((n) => n.kind)).toEqual(["slider", "label"]);
    expect(out).toHaveLength(nodes.length * 2 - 1);
    // Inserted next to the original rather than appended at the end.
    expect(children(out, ROOT_ID).map((n) => n.id)).toEqual([panelId, newId]);
  });

  it("removes a container together with everything inside it", () => {
    const { nodes, panelId } = build();
    expect(removeNode(nodes, panelId)).toHaveLength(1);
    expect(removeNode(nodes, ROOT_ID)).toBe(nodes);
  });
});

describe("select options stay consistent with value", () => {
  it("re-points value when its choice is dropped, and keeps it otherwise", () => {
    let nodes = addNode([createRoot()], ROOT_ID, "select");
    const id = nodes[1].id;
    expect(nodes[1].props.value).toBe("Option 1");
    nodes = setOptions(nodes, id, ["a", "b"]);
    // "Option 1" is gone, so holding it would emit a node gui.rs rejects.
    expect(nodes[1].props.value).toBe("a");
    nodes = updateProps(nodes, id, { value: "b" });
    nodes = setOptions(nodes, id, ["a", "b", "c"]);
    expect(nodes[1].props.value).toBe("b");
  });

  it("emits a list literal the engine's options validator accepts", () => {
    let nodes = addNode([createRoot()], ROOT_ID, "select");
    nodes = setOptions(nodes, nodes[1].id, ['say "hi"', "brace {x}"]);
    const code = designTreeToQu(nodes).join("\n");
    // Quoting and brace-doubling, the same escaping any label gets --
    // an unescaped { would be read as string interpolation.
    expect(code).toContain('options=["say \\"hi\\"", "brace {{x}}"]');
  });
});

describe("generated source", () => {
  it("designTreeToQu emits .on() bindings but never the handler bodies -- those live in form.code.qu", () => {
    let { nodes } = build();
    const slider = nodes.find((n) => n.kind === "slider")!;
    nodes = setEvent(nodes, slider.id, "change", "slider1_change");
    const lines = designTreeToQu(nodes);
    expect(lines.some((l) => l.includes(".on("))).toBe(true);
    expect(lines.some((l) => l.startsWith("function "))).toBe(false);
    expect(lines[lines.length - 1]).toBe("t.show()");
  });

  it("requiredHandlerNames names every distinct bound handler once, in first-seen order", () => {
    let { nodes } = build();
    const slider = nodes.find((n) => n.kind === "slider")!;
    const label = nodes.find((n) => n.kind === "label")!;
    nodes = setEvent(nodes, label.id, "click", "shared_handler");
    nodes = setEvent(nodes, slider.id, "change", "shared_handler");
    expect(requiredHandlerNames(nodes)).toEqual(["shared_handler"]);
  });

  it("reconcileCodeQu adds a stub for a missing handler but never touches one already written", () => {
    const existing = "function existing_click(event)\n  x = 1\nend function\n";
    const updated = reconcileCodeQu(existing, ["existing_click", "new_click"]);
    // The user's real body survives byte-for-byte...
    expect(updated).toContain("function existing_click(event)\n  x = 1\nend function");
    // ...and exactly one new empty stub was appended for the missing name.
    expect(updated).toContain("function new_click(event)\n\nend function");
    expect(updated.match(/function existing_click\(/g)?.length).toBe(1);
  });

  it("reconcileCodeQu leaves a no-longer-required handler's body in place rather than deleting it", () => {
    const existing = "function orphaned_click(event)\n  keep_me = true\nend function\n";
    const updated = reconcileCodeQu(existing, []);
    expect(updated).toBe(existing);
  });

  it("mergeGenScript declares handler functions before the .on() calls that name them", () => {
    let { nodes } = build();
    const slider = nodes.find((n) => n.kind === "slider")!;
    nodes = setEvent(nodes, slider.id, "change", "slider1_change");
    const designQu = designTreeToQu(nodes).join("\n");
    const codeQu = reconcileCodeQu("", requiredHandlerNames(nodes));
    const gen = mergeGenScript(codeQu, designQu);
    const declared = gen.indexOf("function slider1_change");
    const bound = gen.indexOf(".on(");
    // gui.rs: "Define function `{callback}` before registering its handler".
    expect(declared).toBeGreaterThanOrEqual(0);
    expect(declared).toBeLessThan(bound);
    expect(gen.trim().endsWith("t.show()")).toBe(true);
  });

  it("mergeGenScript returns whitespace-only output when it has nothing to merge", () => {
    // Pinned because a host cannot distinguish this from a real script by
    // a null/undefined check. `GuiDesignerPanel` used to hand exactly this
    // to `GuiPanel` whenever Run was pressed before the design had been
    // explicitly synced, and `GuiPanel`'s `initialCode ?? saved ?? example`
    // chain treated the non-empty "\n" as content -- blank editor, Run did
    // nothing, and that was the whole of the "GUI not working" report of
    // 2026-09-16. Hosts must test `.trim()`, not nullishness.
    expect(mergeGenScript("", "").trim()).toBe("");
    expect(mergeGenScript("", "")).not.toBe("");
  });

  it("emits root props other than the title as a .set(), never as Frame kwargs", () => {
    let nodes: DesignNode[] = [createRoot("My app")];
    nodes = updateProps(nodes, ROOT_ID, { layout: "row" });
    const lines = designTreeToQu(nodes);
    // Frame() takes the title positionally and gui.rs overwrites title and
    // visible after applying kwargs -- a layout= there would be dropped.
    expect(lines[0]).toBe('t = Frame("My app")');
    expect(lines[1]).toBe('t.set(layout="row")');
    expect(lines.join("\n")).not.toContain("visible=");
  });

  it("only offers props the engine validates for that kind", () => {
    // x/y are plot-only in gui.rs's validate(); offering them anywhere else
    // would let the inspector build a node the engine refuses.
    const keys = (kind: Parameters<typeof propsFor>[0]) => propsFor(kind).map((p) => p.key);
    expect(keys("plot")).toContain("x");
    expect(keys("button")).not.toContain("x");
    expect(keys("select")).toContain("options");
    expect(keys("label")).not.toContain("options");
    // The root's visible is owned by .show(), so it is not an editable prop.
    expect(keys("frame")).not.toContain("visible");
    expect(keys("panel")).toContain("visible");
  });
});
