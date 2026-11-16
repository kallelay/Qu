import React, { useState } from 'react';
import { AlertCircle, ArrowDown, ArrowUp, Plus, Trash2 } from 'lucide-react';
import { ENVIRONMENTS } from './loopEnvironments';
import {
  CUSTOM_TEMPLATES,
  HOOK_LABEL,
  HOOK_ORDER,
  HOOK_SIGNATURE,
  PLUGIN_LIBRARY,
  buildPluginCheckScript,
  newCustomPlugin,
  newPlugin,
  parsePluginCheck,
  pluginDef,
  pluginProblem,
  type HookKind,
  type PluginInstance,
} from './loopPlugins';

const border = 'var(--qu-border)';
const inputClass = 'px-2 py-1.5 rounded-md text-sm bg-transparent border min-w-0';

interface CheckResult {
  ok: boolean;
  text: string;
}

export interface EnvironmentPickerProps {
  /** The ids of the plant and controller examples now selected (to show the matching card). */
  plantId: string;
  controllerId: string;
  onPick: (environmentId: string) => void;
}

/** A picker of the ready-made environments, and the card that says what the
 *  current one models and what to try on it. */
export const EnvironmentPicker: React.FC<EnvironmentPickerProps> = ({ plantId, controllerId, onPick }) => {
  const current = ENVIRONMENTS.find((e) => e.plant.id === plantId && e.controller.id === controllerId) ?? ENVIRONMENTS.find((e) => e.plant.id === plantId);
  return (
    <div className="flex flex-col gap-1" aria-label="Environments" data-testid="loop-environments">
      <select
        value=""
        onChange={(e) => e.target.value && onPick(e.target.value)}
        className="px-2 py-1 rounded-md text-xs bg-transparent border"
        style={{ borderColor: border }}
        aria-label="Environments"
        title="A plant, a controller, a setpoint and a sample time, ready to run"
      >
        <option value="" disabled>Environments…</option>
        {ENVIRONMENTS.map((e) => <option key={e.id} value={e.id}>{e.label}</option>)}
      </select>
      {current && (
        <div className="text-xs rounded-md p-2 flex flex-col gap-1" style={{ border: `1px solid ${border}` }} data-testid="loop-environment-card">
          <span className="font-medium">{current.label}</span>
          <span style={{ color: 'var(--qu-muted)' }}>{current.physics}</span>
          <span><b>Try:</b> <span style={{ color: 'var(--qu-muted)' }}>{current.tryThis}</span></span>
          <span style={{ color: 'var(--qu-muted)' }}>
            Expected: within {current.expect.band * 100}% of the setpoint {current.expect.settleBy} s after the step{current.expect.unstable ? '; unstable without the controller' : ''}.
          </span>
        </div>
      )}
    </div>
  );
};

export interface PluginsSectionProps {
  plugins: PluginInstance[];
  onChange: (next: PluginInstance[]) => void;
  /** Tauri invoke, to run the check of a custom snippet in a throwaway interpreter. */
  invoke: <T>(cmd: string, args?: Record<string, unknown>) => Promise<T>;
}

/** The Plugins section of the Loop panel: add from the library, enable,
 *  reorder, edit parameters, write your own. Everything it edits ends up as
 *  commented blocks in the generated script. */
export const PluginsSection: React.FC<PluginsSectionProps> = ({ plugins, onChange, invoke }) => {
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  const [open, setOpen] = useState<Record<string, boolean>>({});
  const [checks, setChecks] = useState<Record<string, CheckResult | 'running'>>({});

  const patch = (uid: string, change: Partial<PluginInstance>) => onChange(plugins.map((p) => (p.uid === uid ? { ...p, ...change } : p)));
  const move = (i: number, by: number) => {
    const j = i + by;
    if (j < 0 || j >= plugins.length) return;
    const next = plugins.slice();
    [next[i], next[j]] = [next[j], next[i]];
    onChange(next);
  };
  const add = (pl: PluginInstance) => {
    onChange([...plugins, pl]);
    setOpen((o) => ({ ...o, [pl.uid]: true }));
  };

  const runCheck = async (pl: PluginInstance) => {
    setChecks((c) => ({ ...c, [pl.uid]: 'running' }));
    try {
      const code = buildPluginCheckScript(pl);
      const r = await invoke<{ success: boolean; error: string | null; output?: string }>('execute_code', { request: { code, file_path: null } });
      const parsed = r.success ? parsePluginCheck(r.output ?? '') : null;
      setChecks((c) => ({
        ...c,
        [pl.uid]: parsed?.ok
          ? { ok: true, text: `Ran: the hook returned ${Number.isFinite(parsed.value) ? parsed.value : 'a non-number'} on a test call.` }
          : { ok: false, text: r.error ?? 'The check did not run to the end.' },
      }));
    } catch (err) {
      setChecks((c) => ({ ...c, [pl.uid]: { ok: false, text: err instanceof Error ? err.message : String(err) } }));
    }
  };

  const paramInput = (pl: PluginInstance, id: string, label: string, step?: number) => {
    const key = `${pl.uid}.${id}`;
    return (
      <label key={id} className="flex flex-col gap-1 text-xs flex-1 min-w-[6rem]">
        <span style={{ color: 'var(--qu-muted)' }}>{label}</span>
        <input
          type="number"
          step={step}
          value={drafts[key] ?? pl.params[id]}
          onChange={(e) => {
            const raw = e.target.value;
            setDrafts((d) => ({ ...d, [key]: raw }));
            const v = Number(raw);
            if (raw.trim() !== '' && Number.isFinite(v)) patch(pl.uid, { params: { ...pl.params, [id]: v } });
          }}
          onBlur={() => setDrafts((d) => {
            const { [key]: _, ...rest } = d;
            return rest;
          })}
          className={inputClass}
          style={{ borderColor: border }}
          aria-label={`${pl.uid} ${id}`}
        />
      </label>
    );
  };

  return (
    <div className="flex flex-col gap-2" aria-label="Plugins" data-testid="loop-plugins">
      <div className="flex items-center justify-between gap-2">
        <span className="text-sm font-medium" title="Small Qu snippets hooked into the loop; each is written into the script as a commented block">Plugins</span>
        <select
          value=""
          onChange={(e) => {
            const v = e.target.value;
            if (v === 'custom') add(newCustomPlugin());
            else if (v) add(newPlugin(v));
          }}
          className="px-2 py-1 rounded-md text-xs bg-transparent border"
          style={{ borderColor: border }}
          aria-label="Add plugin"
        >
          <option value="" disabled>Add plugin…</option>
          {HOOK_ORDER.map((h) => (
            <optgroup key={h} label={HOOK_LABEL[h]}>
              {PLUGIN_LIBRARY.filter((d) => d.hook === h).map((d) => <option key={d.id} value={d.id}>{d.label}</option>)}
            </optgroup>
          ))}
          <optgroup label="your own">
            <option value="custom">Write your own…</option>
          </optgroup>
        </select>
      </div>
      {plugins.length === 0 && (
        <span className="text-xs" style={{ color: 'var(--qu-muted)' }}>
          None. A plugin changes the loop at one point: the setpoint, the measurement, the command, the load, or it watches the run.
        </span>
      )}
      {plugins.map((pl, i) => {
        const def = pluginDef(pl.def);
        const hook: HookKind = pl.def === 'custom' ? pl.hook ?? 'sensor' : def?.hook ?? 'sensor';
        const label = pl.def === 'custom' ? pl.name?.trim() || 'custom plugin' : def?.label ?? pl.def;
        const problem = pl.enabled ? pluginProblem(pl) : null;
        const chk = checks[pl.uid];
        return (
          <div key={pl.uid} className="rounded-md text-xs flex flex-col" style={{ border: `1px solid ${problem ? 'var(--qu-danger)' : border}`, opacity: pl.enabled ? 1 : 0.6 }} data-testid="loop-plugin" data-plugin={pl.def}>
            <div className="flex items-center gap-1 px-2 py-1.5">
              <input type="checkbox" checked={pl.enabled} onChange={(e) => patch(pl.uid, { enabled: e.target.checked })} aria-label={`enable ${label}`} title="Switch off to leave it out of the script (keeps its settings)" />
              <button className="flex-1 min-w-0 text-left truncate" onClick={() => setOpen((o) => ({ ...o, [pl.uid]: !o[pl.uid] }))} title={def?.summary ?? 'Your own snippet'} aria-expanded={!!open[pl.uid]}>
                <span className="font-mono" style={{ color: 'var(--qu-muted)' }}>{i + 1}</span> {label}
              </button>
              <span className="px-1 rounded" style={{ border: `1px solid ${border}`, color: 'var(--qu-muted)' }}>{HOOK_LABEL[hook]}</span>
              <button className="p-0.5" onClick={() => move(i, -1)} disabled={i === 0} title="Earlier" aria-label="Move up"><ArrowUp size={12} /></button>
              <button className="p-0.5" onClick={() => move(i, 1)} disabled={i === plugins.length - 1} title="Later" aria-label="Move down"><ArrowDown size={12} /></button>
              <button className="p-0.5" onClick={() => onChange(plugins.filter((p) => p.uid !== pl.uid))} title="Remove" aria-label="Remove plugin"><Trash2 size={12} /></button>
            </div>
            {open[pl.uid] && (
              <div className="flex flex-col gap-2 px-2 pb-2">
                {def && <span style={{ color: 'var(--qu-muted)' }}>{def.summary}</span>}
                {def && (
                  <div className="flex gap-2 flex-wrap">
                    {def.params.map((prm) => paramInput(pl, prm.id, prm.label, prm.step ?? (prm.integer ? 1 : undefined)))}
                  </div>
                )}
                {pl.def === 'custom' && (
                  <>
                    <div className="flex gap-2">
                      <label className="flex flex-col gap-1 flex-1 min-w-0">
                        <span style={{ color: 'var(--qu-muted)' }}>Name</span>
                        <input value={pl.name ?? ''} onChange={(e) => patch(pl.uid, { name: e.target.value })} className={inputClass} style={{ borderColor: border }} aria-label="plugin name" />
                      </label>
                      <label className="flex flex-col gap-1 flex-1 min-w-0">
                        <span style={{ color: 'var(--qu-muted)' }}>Hook point</span>
                        <select
                          value={hook}
                          onChange={(e) => {
                            const h = e.target.value as HookKind;
                            // Switching the hook swaps in that hook's starting snippet only if the text is still a starting snippet.
                            const starting = !pl.code?.trim() || Object.values(CUSTOM_TEMPLATES).includes(pl.code);
                            patch(pl.uid, starting ? { hook: h, code: CUSTOM_TEMPLATES[h] } : { hook: h });
                          }}
                          className={inputClass}
                          style={{ borderColor: border }}
                          aria-label="plugin hook"
                        >
                          {HOOK_ORDER.map((h) => <option key={h} value={h}>{HOOK_LABEL[h]}</option>)}
                        </select>
                      </label>
                    </div>
                    <span style={{ color: 'var(--qu-muted)' }}>
                      Define <code>hook({HOOK_SIGNATURE[hook].args})</code> returning {HOOK_SIGNATURE[hook].returns}. It is written into the script as <code>lp{i + 1}_{HOOK_SIGNATURE[hook].name}</code>. Keep any state in globals whose names start with <code>lp{i + 1}_</code> too.
                    </span>
                    <textarea
                      value={pl.code ?? ''}
                      onChange={(e) => {
                        patch(pl.uid, { code: e.target.value });
                        setChecks((c) => {
                          const { [pl.uid]: _, ...rest } = c;
                          return rest;
                        });
                      }}
                      spellCheck={false}
                      rows={8}
                      className="rounded-md p-2 font-mono text-[11px] leading-snug bg-transparent border w-full"
                      style={{ borderColor: border, whiteSpace: 'pre', overflowX: 'auto' }}
                      aria-label="plugin code"
                    />
                    <div className="flex items-center gap-2">
                      <button className="px-2 py-1 rounded-md border" style={{ borderColor: border }} disabled={!!problem || chk === 'running'} onClick={() => void runCheck(pl)} title="Run the hook on a few test values in a throwaway interpreter">
                        {chk === 'running' ? 'Checking…' : 'Check by running'}
                      </button>
                      {chk && chk !== 'running' && <span style={{ color: chk.ok ? 'var(--qu-ok)' : 'var(--qu-danger)' }}>{chk.ok ? chk.text : 'Failed.'}</span>}
                    </div>
                    {chk && chk !== 'running' && !chk.ok && <pre className="whitespace-pre-wrap break-words font-mono text-[11px]" style={{ color: 'var(--qu-danger)' }}>{chk.text}</pre>}
                  </>
                )}
              </div>
            )}
            {problem && (
              <div className="flex items-start gap-1 px-2 pb-2" style={{ color: 'var(--qu-danger)' }} role="alert">
                <AlertCircle size={12} className="flex-shrink-0 mt-0.5" />
                <span>{problem}</span>
              </div>
            )}
          </div>
        );
      })}
      {plugins.length > 0 && (
        <button className="flex items-center gap-1 text-xs self-start" style={{ color: 'var(--qu-accent)' }} onClick={() => add(newCustomPlugin())}>
          <Plus size={12} /> Write your own
        </button>
      )}
    </div>
  );
};
