import React, { useEffect, useState } from 'react';
import { motion, AnimatePresence } from 'framer-motion';
import { X, Sparkles, Loader, Check, AlertCircle } from 'lucide-react';
import { cn } from '@qu/ui-components';

/**
 * Settings panel for QuStudio's "AI Assist" backend -- pick Local
 * (TinyLlama, offline, ships with the app) or a hosted provider
 * (OpenAI/Anthropic), paste an API key, and test it before saving. Mirrors
 * `AiDiffModal`'s modal-over-backdrop styling (same color tokens, same
 * framer-motion entrance) since that's the one existing "floating panel
 * over the editor" pattern in this app -- there's no pre-existing settings
 * surface elsewhere to match instead (grepped for "Settings"/"Preferences"
 * panels before writing this; there are none).
 *
 * Talks to three Tauri commands added alongside this component (see
 * `llm_bridge.rs`): `get_llm_provider_config`, `set_llm_provider_config`,
 * `test_llm_provider`. Never receives a real saved API key back from the
 * Rust side -- `get_llm_provider_config`'s response only ever says whether
 * one is saved (`hasOpenaiKey`/`hasAnthropicKey`), never the value itself
 * (see `llm_providers::PublicProviderSettings` on the Rust side). This
 * component's own local `openaiKeyInput`/`anthropicKeyInput` state holds
 * whatever the user is CURRENTLY typing, which is separate from, and never
 * pre-filled from, whatever's already saved.
 */

export type ProviderKind = 'local' | 'openai' | 'anthropic';

interface PublicProviderSettings {
  provider: ProviderKind;
  openaiModel: string;
  anthropicModel: string;
  hasOpenaiKey: boolean;
  hasAnthropicKey: boolean;
}

export interface LlmProviderSettingsProps {
  open: boolean;
  onClose: () => void;
  theme?: 'light' | 'dark';
  invoke: <T,>(command: string, args?: Record<string, any>) => Promise<T>;
}

type TestState = { status: 'idle' } | { status: 'testing' } | { status: 'ok' } | { status: 'error'; message: string };

const PROVIDERS: { id: ProviderKind; label: string; blurb: string }[] = [
  { id: 'local', label: 'Local (TinyLlama)', blurb: 'Runs on this machine, CPU-only, fully offline. No key needed.' },
  { id: 'openai', label: 'OpenAI', blurb: 'Chat Completions API -- needs an API key from platform.openai.com.' },
  { id: 'anthropic', label: 'Anthropic', blurb: 'Messages API -- needs an API key from console.anthropic.com.' },
];

export const LlmProviderSettings: React.FC<LlmProviderSettingsProps> = ({ open, onClose, theme = 'dark', invoke }) => {
  const dark = theme === 'dark';
  const [loaded, setLoaded] = useState(false);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [provider, setProvider] = useState<ProviderKind>('local');
  const [openaiModel, setOpenaiModel] = useState('gpt-4o-mini');
  const [anthropicModel, setAnthropicModel] = useState('claude-haiku-4-5');
  const [hasOpenaiKey, setHasOpenaiKey] = useState(false);
  const [hasAnthropicKey, setHasAnthropicKey] = useState(false);
  const [openaiKeyInput, setOpenaiKeyInput] = useState('');
  const [anthropicKeyInput, setAnthropicKeyInput] = useState('');
  const [testState, setTestState] = useState<TestState>({ status: 'idle' });
  const [saveState, setSaveState] = useState<'idle' | 'saving' | 'saved' | 'error'>('idle');
  const [saveError, setSaveError] = useState<string | null>(null);

  // Reload from disk every time the panel opens, rather than once at app
  // startup -- another QuStudio window/process (or a future multi-window
  // build) could have changed the saved provider since this panel was last
  // shown, and the settings file is the source of truth, not this
  // component's own memory of it.
  useEffect(() => {
    if (!open) return;
    setLoaded(false);
    setLoadError(null);
    setTestState({ status: 'idle' });
    setSaveState('idle');
    setOpenaiKeyInput('');
    setAnthropicKeyInput('');
    invoke<PublicProviderSettings>('get_llm_provider_config')
      .then((cfg) => {
        setProvider(cfg.provider);
        setOpenaiModel(cfg.openaiModel);
        setAnthropicModel(cfg.anthropicModel);
        setHasOpenaiKey(cfg.hasOpenaiKey);
        setHasAnthropicKey(cfg.hasAnthropicKey);
        setLoaded(true);
      })
      .catch((err: any) => {
        setLoadError(err?.message ?? String(err));
        setLoaded(true);
      });
  }, [open, invoke]);

  if (!open) return null;

  const handleTest = async () => {
    setTestState({ status: 'testing' });
    try {
      await invoke<void>('test_llm_provider', {
        request: {
          provider,
          api_key: provider === 'openai' ? openaiKeyInput || null : provider === 'anthropic' ? anthropicKeyInput || null : null,
          model: provider === 'openai' ? openaiModel : provider === 'anthropic' ? anthropicModel : null,
        },
      });
      setTestState({ status: 'ok' });
    } catch (err: any) {
      setTestState({ status: 'error', message: err?.message ?? String(err) });
    }
  };

  const handleSave = async () => {
    setSaveState('saving');
    setSaveError(null);
    try {
      const cfg = await invoke<PublicProviderSettings>('set_llm_provider_config', {
        update: {
          provider,
          openai_model: openaiModel,
          anthropic_model: anthropicModel,
          // Empty input means "don't touch the saved key" -- only send a
          // key when the user actually typed something this session (see
          // `ProviderSettingsUpdate`'s own doc comment on the Rust side for
          // the None-keeps/Some("")-clears semantics this maps onto).
          openai_api_key: openaiKeyInput ? openaiKeyInput : null,
          anthropic_api_key: anthropicKeyInput ? anthropicKeyInput : null,
        },
      });
      setHasOpenaiKey(cfg.hasOpenaiKey);
      setHasAnthropicKey(cfg.hasAnthropicKey);
      setOpenaiKeyInput('');
      setAnthropicKeyInput('');
      setSaveState('saved');
    } catch (err: any) {
      setSaveError(err?.message ?? String(err));
      setSaveState('error');
    }
  };

  const handleClearKey = async (which: 'openai' | 'anthropic') => {
    setSaveState('saving');
    setSaveError(null);
    try {
      const cfg = await invoke<PublicProviderSettings>('set_llm_provider_config', {
        update: {
          provider,
          openai_model: openaiModel,
          anthropic_model: anthropicModel,
          openai_api_key: which === 'openai' ? '' : null,
          anthropic_api_key: which === 'anthropic' ? '' : null,
        },
      });
      setHasOpenaiKey(cfg.hasOpenaiKey);
      setHasAnthropicKey(cfg.hasAnthropicKey);
      setSaveState('saved');
    } catch (err: any) {
      setSaveError(err?.message ?? String(err));
      setSaveState('error');
    }
  };

  return (
    <AnimatePresence>
      <motion.div
        initial={{ opacity: 0 }}
        animate={{ opacity: 1 }}
        exit={{ opacity: 0 }}
        className="fixed inset-0 z-[70] flex items-center justify-center bg-black/50 p-6"
        onClick={onClose}
      >
        <motion.div
          initial={{ opacity: 0, y: 12, scale: 0.97 }}
          animate={{ opacity: 1, y: 0, scale: 1 }}
          exit={{ opacity: 0, y: 12, scale: 0.97 }}
          transition={{ duration: 0.15 }}
          onClick={(e) => e.stopPropagation()}
          className={cn(
            'w-full max-w-lg max-h-[85vh] rounded-xl shadow-2xl flex flex-col overflow-hidden border',
            dark ? 'bg-[#161615] border-[#2c2c2a] text-[#dfe7f2]' : 'bg-white border-[#e1e0d9] text-[#52514e]'
          )}
        >
          <div className={cn('flex items-center justify-between px-4 py-3 border-b flex-shrink-0', dark ? 'border-[#2c2c2a]' : 'border-[#e1e0d9]')}>
            <div className="flex items-center gap-2">
              <Sparkles size={16} className={dark ? 'text-[#3987e5]' : 'text-[#2a78d6]'} />
              <div className="font-semibold text-sm">AI Provider Settings</div>
            </div>
            <button onClick={onClose} className={cn('p-1 rounded transition-colors', dark ? 'hover:bg-[#2c2c2a]' : 'hover:bg-[#e1e0d9]')} aria-label="Close">
              <X size={14} />
            </button>
          </div>

          <div className="flex-1 overflow-auto px-4 py-3 text-sm space-y-4">
            {!loaded ? (
              <div className={cn('flex items-center gap-2 py-6 justify-center', dark ? 'text-[#898781]' : 'text-[#898781]')}>
                <Loader size={16} className="animate-spin" /> Loading current settings...
              </div>
            ) : loadError ? (
              <div className="rounded-lg px-3 py-2 bg-red-500/15 text-red-400 border border-red-500/30 text-xs">
                Could not load settings: {loadError}
              </div>
            ) : (
              <>
                <div className="space-y-2">
                  {PROVIDERS.map((p) => (
                    <label
                      key={p.id}
                      className={cn(
                        'flex items-start gap-2 rounded-lg border px-3 py-2 cursor-pointer transition-colors',
                        provider === p.id
                          ? dark
                            ? 'border-[#3987e5] bg-[#3987e5]/10'
                            : 'border-[#2a78d6] bg-[#2a78d6]/10'
                          : dark
                          ? 'border-[#2c2c2a] hover:bg-[#2c2c2a]/50'
                          : 'border-[#e1e0d9] hover:bg-[#e1e0d9]/50'
                      )}
                    >
                      <input
                        type="radio"
                        name="llm-provider"
                        className="mt-0.5"
                        checked={provider === p.id}
                        onChange={() => {
                          setProvider(p.id);
                          setTestState({ status: 'idle' });
                        }}
                      />
                      <div>
                        <div className="font-medium">{p.label}</div>
                        <div className={cn('text-xs', dark ? 'text-[#898781]' : 'text-[#898781]')}>{p.blurb}</div>
                      </div>
                    </label>
                  ))}
                </div>

                {provider === 'openai' && (
                  <div className="space-y-2">
                    <label className="block text-xs font-medium">Model</label>
                    <input
                      className={cn('w-full rounded-lg border px-2 py-1.5 text-sm bg-transparent', dark ? 'border-[#2c2c2a]' : 'border-[#e1e0d9]')}
                      value={openaiModel}
                      onChange={(e) => setOpenaiModel(e.target.value)}
                    />
                    <label className="block text-xs font-medium">
                      API key {hasOpenaiKey && <span className="text-emerald-500">(saved)</span>}
                    </label>
                    <div className="flex gap-2">
                      <input
                        type="password"
                        placeholder={hasOpenaiKey ? 'Leave blank to keep the saved key' : 'sk-...'}
                        className={cn('flex-1 rounded-lg border px-2 py-1.5 text-sm bg-transparent', dark ? 'border-[#2c2c2a]' : 'border-[#e1e0d9]')}
                        value={openaiKeyInput}
                        onChange={(e) => setOpenaiKeyInput(e.target.value)}
                        autoComplete="off"
                      />
                      {hasOpenaiKey && (
                        <button
                          onClick={() => handleClearKey('openai')}
                          className={cn('px-2 rounded-lg text-xs border', dark ? 'border-[#2c2c2a] hover:bg-[#2c2c2a]' : 'border-[#e1e0d9] hover:bg-[#e1e0d9]')}
                        >
                          Remove
                        </button>
                      )}
                    </div>
                  </div>
                )}

                {provider === 'anthropic' && (
                  <div className="space-y-2">
                    <label className="block text-xs font-medium">Model</label>
                    <input
                      className={cn('w-full rounded-lg border px-2 py-1.5 text-sm bg-transparent', dark ? 'border-[#2c2c2a]' : 'border-[#e1e0d9]')}
                      value={anthropicModel}
                      onChange={(e) => setAnthropicModel(e.target.value)}
                    />
                    <label className="block text-xs font-medium">
                      API key {hasAnthropicKey && <span className="text-emerald-500">(saved)</span>}
                    </label>
                    <div className="flex gap-2">
                      <input
                        type="password"
                        placeholder={hasAnthropicKey ? 'Leave blank to keep the saved key' : 'sk-ant-...'}
                        className={cn('flex-1 rounded-lg border px-2 py-1.5 text-sm bg-transparent', dark ? 'border-[#2c2c2a]' : 'border-[#e1e0d9]')}
                        value={anthropicKeyInput}
                        onChange={(e) => setAnthropicKeyInput(e.target.value)}
                        autoComplete="off"
                      />
                      {hasAnthropicKey && (
                        <button
                          onClick={() => handleClearKey('anthropic')}
                          className={cn('px-2 rounded-lg text-xs border', dark ? 'border-[#2c2c2a] hover:bg-[#2c2c2a]' : 'border-[#e1e0d9] hover:bg-[#e1e0d9]')}
                        >
                          Remove
                        </button>
                      )}
                    </div>
                  </div>
                )}

                {provider !== 'local' && (
                  <div className="flex items-center gap-2">
                    <button
                      onClick={handleTest}
                      disabled={testState.status === 'testing'}
                      className={cn(
                        'flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs font-medium border transition-colors disabled:opacity-50',
                        dark ? 'border-[#2c2c2a] hover:bg-[#2c2c2a]' : 'border-[#e1e0d9] hover:bg-[#e1e0d9]'
                      )}
                    >
                      {testState.status === 'testing' ? <Loader size={12} className="animate-spin" /> : null}
                      Test connection
                    </button>
                    {testState.status === 'ok' && (
                      <span className="flex items-center gap-1 text-xs text-emerald-500">
                        <Check size={12} /> Key works
                      </span>
                    )}
                    {testState.status === 'error' && (
                      <span className="flex items-center gap-1 text-xs text-red-400">
                        <AlertCircle size={12} /> {testState.message}
                      </span>
                    )}
                  </div>
                )}

                <p className={cn('text-xs', dark ? 'text-[#898781]' : 'text-[#898781]')}>
                  If the selected hosted provider fails (bad key, no network, rate limit), Qu Studio automatically
                  falls back to the local model instead of breaking AI Assist -- you'll see which backend actually
                  answered in the response.
                </p>
              </>
            )}
          </div>

          <div className={cn('flex items-center justify-between px-4 py-3 border-t flex-shrink-0', dark ? 'border-[#2c2c2a]' : 'border-[#e1e0d9]')}>
            <span className="text-xs">
              {saveState === 'saving' && 'Saving...'}
              {saveState === 'saved' && <span className="text-emerald-500">Saved</span>}
              {saveState === 'error' && <span className="text-red-400">{saveError}</span>}
            </span>
            <div className="flex gap-2">
              <button
                onClick={onClose}
                className={cn('px-3 py-1.5 rounded-lg text-sm font-medium transition-colors', dark ? 'hover:bg-[#2c2c2a]' : 'hover:bg-[#e1e0d9]')}
              >
                Close
              </button>
              <button
                onClick={handleSave}
                disabled={!loaded || saveState === 'saving'}
                className="px-3 py-1.5 rounded-lg text-sm font-medium bg-[#2a78d6] text-white hover:bg-[#3987e5] disabled:opacity-50"
              >
                Save
              </button>
            </div>
          </div>
        </motion.div>
      </motion.div>
    </AnimatePresence>
  );
};

export default LlmProviderSettings;
