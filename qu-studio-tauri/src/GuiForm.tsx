import React from 'react';
import { PlotViewer } from '@qu/ui-components';

export interface GuiNode { id: string; parent: string | null; kind: string; props: Record<string, any>; events: string[] }

interface GuiFormProps {
  nodes: GuiNode[];
  running: boolean;
  pending: boolean;
  theme: 'light' | 'dark';
  onEvent: (node: GuiNode, event: string, value?: unknown) => void;
  /** Extra action for a frame's `close` handler -- the popup runner window
   *  closes itself here; an inline embed has no window to close. */
  onFrameClose?: () => void;
}

/** Renders a `gui.rs` node tree as real, styled, interactive controls.
 *  Shared by the popup runner window (`GuiRunnerWindow.tsx`) and the
 *  inline embed (`GuiPanel.tsx`'s `mode="inline"`) so a script's form
 *  looks and behaves the same whichever way it's run -- the two used to
 *  drift independently (the popup rendered bare unstyled HTML while
 *  GuiDesigner's own canvas showed a fully dressed mockup of the same
 *  tree; "these two things are totally different", 2026-09-17). */
export function GuiForm({ nodes, running, pending, theme, onEvent, onFrameClose }: GuiFormProps) {
  function render(node: GuiNode): React.ReactNode {
    const p = node.props;
    if (p.visible === false) return null;
    const disabled = !!p.disabled || pending || !running;
    const children = nodes.filter(child => child.parent === node.id).map(render);
    const input = { disabled: disabled || !node.events.includes('change'), 'aria-label': p.text ?? node.kind };
    const change = (value: unknown) => onEvent(node, 'change', value);
    // `gui.rs`'s own layout switch (row/column/grid), kept in lockstep
    // with `GuiDesigner.tsx`'s `renderPreview` container styling.
    const containerStyle: React.CSSProperties = { display: p.layout === 'grid' ? 'grid' : 'flex', flexDirection: p.layout === 'row' ? 'row' : 'column', gridTemplateColumns: p.layout === 'grid' ? 'repeat(auto-fit,minmax(200px,1fr))' : undefined, gap: 16, flexWrap: 'wrap', alignItems: 'stretch' };
    let content: React.ReactNode;
    switch (node.kind) {
      // Only the root node is ever a `frame` -- `gui.rs` rejects
      // `.add("frame", ...)` -- so every other container is a `panel`.
      case 'frame':
        content = (
          <div className="qu-gui-form">
            <div className="qu-gui-titlebar">
              <h2>{p.title}</h2>
              {node.events.includes('close') && <button className="qu-gui-button is-close" disabled={disabled} onClick={() => { onEvent(node, 'close'); onFrameClose?.(); }}>Close</button>}
            </div>
            <div className="qu-gui-body" style={containerStyle}>{children}</div>
          </div>
        );
        break;
      case 'panel':
        content = (
          <div className="qu-gui-panel">
            {typeof p.title === 'string' && p.title && <p className="qu-gui-panel-title">{p.title}</p>}
            <div style={containerStyle}>{children}</div>
          </div>
        );
        break;
      case 'label': content = <span className="qu-gui-label">{p.text}</span>; break;
      case 'button': content = <button className="qu-gui-button" disabled={disabled || !node.events.includes('click')} onClick={() => onEvent(node, 'click')}>{p.text ?? 'Button'}</button>; break;
      case 'slider': case 'number':
        content = (
          <label className="qu-gui-field">
            <span>{p.text ?? node.kind}</span>
            <span style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
              <input {...input} type={node.kind === 'slider' ? 'range' : 'number'} min={p.min ?? 0} max={p.max ?? 100} step="any" value={p.value ?? p.min ?? 0} onChange={event => { if (event.target.value !== '') change(Number(event.target.value)); }} style={node.kind === 'slider' ? { flex: 1 } : { width: 90 }} />
              <output className="qu-gui-output">{p.value ?? p.min ?? 0}</output>
            </span>
          </label>
        );
        break;
      case 'checkbox': content = <label className="qu-gui-checkbox"><input {...input} type="checkbox" checked={p.value ?? false} onChange={event => change(event.target.checked)} />{p.text ?? 'Checkbox'}</label>; break;
      case 'text': content = <label className="qu-gui-field"><span>{p.text ?? 'Text'}</span><input {...input} type="text" value={p.value ?? ''} onChange={event => change(event.target.value)} /></label>; break;
      case 'select': content = <label className="qu-gui-field"><span>{p.text ?? 'Select'}</span><select {...input} value={p.value ?? ''} onChange={event => change(event.target.value)}>{(Array.isArray(p.options) ? p.options : []).map((option: string) => <option key={option} value={option}>{option}</option>)}</select></label>; break;
      case 'plot': content = <PlotViewer title={p.title} xlabel={p.xlabel} ylabel={p.ylabel} equalAspect={p.equal_aspect} theme={theme} animate={false} showLegend={false} data={{ uid: node.id, type:'line', x:p.x ?? [], y:p.y ?? [] }} height={420} />; break;
      default: content = <span>Unsupported widget: {node.kind}</span>;
    }
    return <div key={node.id} style={{minWidth:0,flex:node.kind === 'plot' ? '1 1 100%' : undefined}}>{content}</div>;
  }
  return <>{nodes.filter(node => node.parent === null).map(render)}</>;
}

export default GuiForm;
