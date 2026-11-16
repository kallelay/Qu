import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it, vi } from 'vitest';

// The panel's Tauri imports are only used inside handlers; a static render never reaches them.
vi.mock('@tauri-apps/api/event', () => ({ listen: async () => () => undefined }));

import { EnvironmentPicker, PluginsSection } from './LoopPluginsPanel';
import { LoopPanel } from './LoopPanel';
import { ENVIRONMENTS } from './loopEnvironments';
import { PLUGIN_LIBRARY, newCustomPlugin, newPlugin } from './loopPlugins';

const esc = (t: string) => t.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/'/g, '&#x27;');
const invoke = (async () => ({ success: true, error: null, output: '' })) as never;

describe('EnvironmentPicker', () => {
  it('lists every environment and describes the one that is selected', () => {
    const env = ENVIRONMENTS[3];
    const html = renderToStaticMarkup(<EnvironmentPicker plantId={env.plant.id} controllerId={env.controller.id} onPick={() => undefined} />);
    for (const e of ENVIRONMENTS) expect(html).toContain(esc(e.label));
    expect(html).toContain('loop-environment-card');
    expect(html).toContain('unstable without the controller');
    expect(html).toContain('Try:');
  });

  it('shows no card when the plant is not an environment', () => {
    const html = renderToStaticMarkup(<EnvironmentPicker plantId="lag1" controllerId="pid" onPick={() => undefined} />);
    expect(html).not.toContain('loop-environment-card');
  });
});

describe('PluginsSection', () => {
  it('offers the whole library grouped by hook, plus Write your own', () => {
    const html = renderToStaticMarkup(<PluginsSection plugins={[]} onChange={() => undefined} invoke={invoke} />);
    for (const d of PLUGIN_LIBRARY) expect(html).toContain(esc(d.label));
    expect(html).toContain('Write your own');
    expect(html).toContain('None.');
  });

  it('lists the plugins in order with their hook, and flags an invalid one', () => {
    const bad = newPlugin('sensor-delay');
    bad.params = { n: 0 };
    const html = renderToStaticMarkup(<PluginsSection plugins={[newPlugin('sensor-noise'), bad, newCustomPlugin('monitor')]} onChange={() => undefined} invoke={invoke} />);
    expect(html.match(/data-testid="loop-plugin"/g)?.length).toBe(3);
    expect(html).toContain('Sensor noise');
    expect(html).toContain('at least 1');
    expect(html).toContain('my plugin');
  });
});

describe('LoopPanel first render', () => {
  it('shows the environment picker and the Plugins section, in both themes', () => {
    for (const theme of ['light', 'dark'] as const) {
      const html = renderToStaticMarkup(<LoopPanel theme={theme} invoke={invoke} />);
      expect(html).toContain('loop-environments');
      expect(html).toContain('loop-plugins');
      expect(html).toContain('data-theme="' + theme + '"');
      expect(html).toContain('# Loop, SIL');
    }
  });
});
