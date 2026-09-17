import React, { useEffect, useMemo, useRef, useState } from 'react';
import { CodeEditor, FigureViewer } from '@qu/ui-components';
import { AlertCircle, Play, Radio, Waves, SlidersHorizontal } from 'lucide-react';
import type { LiveSerialPlotPanelProps } from './LiveSerialPlotPanel';

/** Matches `ExecuteResponse` in `main.rs` -- only the fields this panel
 *  actually reads. */
interface ExecuteResult {
  success: boolean;
  output: string;
  error: string | null;
  plots: string[];
}

// The token is declared at :root now (see inspector.css), so the literal
// fallback is genuinely unreachable rather than quietly-wrong: it used to
// say #2c2c2a, which is the SHELL border, not the panel border this
// actually resolves to.
const border = 'var(--qu-border)';

/** The Spectrum/Waveform mode's editable half: signal generation only.
 *  The workbench appends a fixed plotting footer (below) referencing the
 *  variables this convention establishes (`x`, `sig`, `fs`) -- the same
 *  "generate real Qu, run it for real" model as the rest of QuStudio's
 *  tools, rather than a second, hand-rolled FFT/plot implementation here. */
const DEFAULT_SIGNAL = `# Edit the signal below -- Run adds the waveform +
# spectrum plots automatically (x, sig, fs are the
# names it looks for).
fs = 1000
x = 0 to 1 step 1/fs
sig = sin(2*pi*50*x) + 0.5*sin(2*pi*120*x)
`;

/** Complete, standalone DSP demo scripts -- these already call their own
 *  `subplot`/`plot`/`title`, so loading one runs it verbatim rather than
 *  through the `x`/`sig`/`fs` wrapper below. Relocated here 2026-09-16 from
 *  the old DSP-tab sidebar (which shared Code's layout and just inserted
 *  these as a plain template list) -- real, substantive examples, worth
 *  keeping rather than dropping when that sidebar went away. */
const SPECTRUM_EXAMPLES: Array<{ name: string; code: string }> = [
  {
    name: 'Window Functions',
    code: `# Window Functions Comparison
N = 64
n = 0 to N-1

# Different window functions
rect = ones(N)
hann = 0.5 * (1 - cos(2*pi*n/(N-1)))
hamming = 0.54 - 0.46*cos(2*pi*n/(N-1))
blackman = 0.42 - 0.5*cos(2*pi*n/(N-1)) + 0.08*cos(4*pi*n/(N-1))

# Plot windows
subplot(2, 1, 1)
plot(n, rect, label='Rectangular')
plot(n, hann, label='Hann')
plot(n, hamming, label='Hamming')
plot(n, blackman, label='Blackman')
title('Window Functions')
legend
grid on

# Frequency responses
subplot(2, 1, 2)
W_rect = 20*log10(abs(fft(rect, 1024)))
W_hann = 20*log10(abs(fft(hann, 1024)))
W_hamming = 20*log10(abs(fft(hamming, 1024)))
W_blackman = 20*log10(abs(fft(blackman, 1024)))

f = 0 to 511
plot(f, W_rect[1:512], label='Rectangular')
plot(f, W_hann[1:512], label='Hann')
plot(f, W_hamming[1:512], label='Hamming')
plot(f, W_blackman[1:512], label='Blackman')
title('Frequency Response')
xlabel('Normalized Frequency')
ylabel('Magnitude (dB)')
legend
grid on`,
  },
  {
    name: 'PSD Estimation',
    code: `# Power Spectral Density Estimation
Fs = 1000
T = 5
t = 0 to T step 1/Fs

# Signal with noise
x = sin(2*pi*50*t) + 0.5*sin(2*pi*120*t) + randn(length(t)) * 0.5

# Periodogram method
N = length(x)
X = fft(x)
Pxx = (1/(N*Fs)) * abs(X).^2
f = (0:N-1) * Fs / N

# N can be odd (T=5, Fs=1000 gives 5001 samples), so N/2 isn't always an
# integer index — floor it. Qu is 0-indexed throughout, so the DC bin is
# index 0 and half[0:halfN] is the [0, halfN) index range (half-open slice).
halfN = floor(N/2)

# Plot PSD
plot(f[0:halfN], 10*log10(Pxx[0:halfN]))
title('Power Spectral Density (Periodogram)')
xlabel('Frequency (Hz)')
ylabel('Power/Frequency (dB/Hz)')
grid on

# Find peaks
fp = findpeaks(10*log10(Pxx[0:halfN]), min_peak_height=-20)
peaks = fp.peaks
locations = fp.locations
print "Detected frequencies: {f[locations]} Hz"`,
  },
  {
    name: 'Wavelet Transform',
    code: `# Discrete Wavelet Transform
# Load or generate signal. Length must be a power of two so 4 levels of
# Haar decomposition each cleanly halve it (1024 -> 512 -> 256 -> 128 -> 64).
Fs = 1000
N = 1024
t = (0:N-1) / Fs
# Masks can index a vector (x[mask] = ...) but can't be used directly in
# arithmetic like .* — build the piecewise signal via mask assignment instead.
x = sin(2*pi*50*t)
x[t >= 0.5] = sin(2*pi*100*t)[t >= 0.5]

# Multi-level Haar DWT. Qu only implements single-level Haar dwt/idwt (see
# book/src/stdlib/signal-processing.md — no wavedec/wrcoef, no Daubechies),
# so a 4-level decomposition chains dwt on the running approximation band,
# the same thing MATLAB's wavedec does internally. dwt(x) returns a (2, N/2)
# matrix: row 0 is the approximation band, row 1 is the detail band.
lvl1 = dwt(x)
A1 = lvl1[0, :]
D1 = lvl1[1, :]

lvl2 = dwt(A1)
A2 = lvl2[0, :]
D2 = lvl2[1, :]

lvl3 = dwt(A2)
A3 = lvl3[0, :]
D3 = lvl3[1, :]

lvl4 = dwt(A3)
A4 = lvl4[0, :]
D4 = lvl4[1, :]

# Plot decomposition
subplot(5, 1, 1)
plot(A4)
title('A4 (Approximation)')

subplot(5, 1, 2)
plot(D1)
title('D1 (Detail, High Freq)')

subplot(5, 1, 3)
plot(D2)
title('D2')

subplot(5, 1, 4)
plot(D3)
title('D3')

subplot(5, 1, 5)
plot(D4)
title('D4 (Detail, Low Freq)')`,
  },
];

function spectrumScript(signalSource: string): string {
  return `${signalSource}
subplot(2, 1, 1)
plot(x, sig)
title("Waveform")
xlabel("Time (s)")
ylabel("Amplitude")

subplot(2, 1, 2)
X = rfft(sig)
f = (0 to length(X)-1) * fs / length(sig)
plot(f, abs(X))
title("Spectrum")
xlabel("Frequency (Hz)")
ylabel("|X|")
`;
}

type FilterKind = 'low' | 'high' | 'band' | 'stop';

function filterScript(order: number, kind: FilterKind, f1: number, f2: number, fs: number): string {
  const cutoff = kind === 'band' || kind === 'stop' ? `[${f1}, ${f2}]` : `${f1}`;
  return `filt = butter(${order}, "${kind}", ${cutoff}, ${fs})\nfilt.fvtool()\n`;
}

/** DSP workbench: Ahmed's own ruling (2026-09-16) on what the DSP tab
 *  should be, once it turned out to be byte-identical to Code otherwise --
 *  a live spectrum/waveform panel AND an interactive filter designer.
 *  "Online" (continuous streaming) is scoped out of this pass for both
 *  modes -- see docs/design (this session's plan) for why: shipping
 *  Offline complete beats rushing a half-working live path. Both modes
 *  never compute DSP math themselves; they always run real Qu through
 *  `execute_code` and show the resulting figure. */
export const DspWorkbenchPanel: React.FC<LiveSerialPlotPanelProps> = ({ theme, invoke }) => {
  const [mode, setMode] = useState<'spectrum' | 'filter'>('spectrum');

  return (
    <div className="qu-inspector flex flex-1 flex-col min-w-0 min-h-0" data-theme={theme}>
      <div className="qu-live-tabs" role="tablist" aria-label="DSP workbench">
        <button role="tab" aria-selected={mode === 'spectrum'} onClick={() => setMode('spectrum')}>
          <Waves size={12} style={{ verticalAlign: '-2px', marginRight: 5 }} />
          Spectrum / Waveform
        </button>
        <button role="tab" aria-selected={mode === 'filter'} onClick={() => setMode('filter')}>
          <SlidersHorizontal size={12} style={{ verticalAlign: '-2px', marginRight: 5 }} />
          Filter Designer
        </button>
      </div>
      <div className="flex flex-1 min-h-0 min-w-0">
        {mode === 'spectrum' ? <SpectrumMode theme={theme} invoke={invoke} /> : <FilterMode theme={theme} invoke={invoke} />}
      </div>
    </div>
  );
};

/** Shared "run this Qu source, show the resulting figures" plumbing --
 *  both modes generate different source but need the identical
 *  execute/error/plots handling, so it lives once. */
function useRunOnce(invoke: LiveSerialPlotPanelProps['invoke']) {
  const [plots, setPlots] = useState<string[]>([]);
  const [error, setError] = useState('');
  const [running, setRunning] = useState(false);
  const run = async (code: string) => {
    setRunning(true);
    setError('');
    try {
      const result = await invoke<ExecuteResult>('execute_code', { request: { code, file_path: null } });
      if (!result.success) setError(result.error ?? 'Run failed');
      setPlots(result.plots ?? []);
    } catch (err) {
      setError(String(err));
    } finally {
      setRunning(false);
    }
  };
  return { plots, error, running, run };
}

/** A dashed-outline box reads as a drop target or a disabled button --
 *  two things a user might try to interact with. This is neither: it is a
 *  note about scope, sitting beside the panel title where the eye lands
 *  first. Same words, no box, muted, so it informs without competing
 *  with the control it sits next to. */
const OnlineComingSoon: React.FC = () => (
  <span
    style={{ display: 'inline-flex', alignItems: 'center', gap: 5, fontSize: 11, color: 'var(--qu-muted)' }}
    title="Continuous streaming mode -- scoped out of this pass, offline (one-shot) is complete"
  >
    <Radio size={11} />
    Online: coming soon
  </span>
);

const SpectrumMode: React.FC<LiveSerialPlotPanelProps> = ({ theme, invoke }) => {
  const [source, setSource] = useState(DEFAULT_SIGNAL);
  // Examples already call their own subplot/plot -- Run should send them
  // straight to `execute_code`, not through the x/sig/fs wrapper meant for
  // the plain-signal default. Editing the box after loading one flips this
  // back, since a hand-edit is no longer necessarily a complete script.
  const [runVerbatim, setRunVerbatim] = useState(false);
  const { plots, error, running, run } = useRunOnce(invoke);

  return (
    <>
      <div className="w-[26rem] flex-shrink-0 border-r overflow-y-auto flex flex-col gap-3 p-3" style={{ borderColor: border }}>
        <div className="flex items-center justify-between">
          <span className="text-sm font-medium">Signal</span>
          <OnlineComingSoon />
        </div>
        <select
          value=""
          onChange={(e) => {
            const example = SPECTRUM_EXAMPLES.find((x) => x.name === e.target.value);
            if (example) {
              setSource(example.code);
              setRunVerbatim(true);
            }
          }}
          className="px-2 py-1.5 rounded-md text-xs bg-transparent border"
          style={{ borderColor: border }}
        >
          <option value="" disabled>
            Load an example…
          </option>
          {SPECTRUM_EXAMPLES.map((x) => (
            <option key={x.name} value={x.name}>
              {x.name}
            </option>
          ))}
        </select>
        {/* Grows with the panel instead of sitting at a fixed 18rem with
            a column of dead space under it -- this editor is the one
            thing on this side you actually work in. */}
        <div className="flex-1 min-h-[14rem] rounded-xl overflow-hidden border" style={{ borderColor: border }}>
          <CodeEditor
            language="qu"
            value={source}
            onChange={(v) => {
              setSource(v);
              setRunVerbatim(false);
            }}
            theme={theme}
            showMinimap={false}
          />
        </div>
        <button
          onClick={() => void run(runVerbatim ? source : spectrumScript(source))}
          disabled={running}
          className="qu-side-action w-full"
        >
          <Play size={13} /> {running ? 'Running…' : 'Run'}
        </button>
        {error && (
          <div className="qu-banner" data-kind="error" style={{ borderRadius: 8, borderTopWidth: 1, borderStyle: 'solid', alignItems: 'flex-start', fontSize: 12.5, padding: '8px 10px' }}>
            <AlertCircle size={14} className="flex-shrink-0 mt-0.5" />
            <span className="break-words">{error}</span>
          </div>
        )}
        <p className="text-xs" style={{ color: 'var(--qu-muted)' }}>
          Run adds the waveform (top) and spectrum via <code>rfft</code> (bottom) automatically from{' '}
          <code>x</code>/<code>sig</code>/<code>fs</code> -- edit the signal above, everything else is generated.
        </p>
      </div>
      <div className="flex-1 min-w-0 overflow-auto p-3">
        <FigureViewer
          images={plots}
          theme={theme}
          emptyHint={<>Press <strong>Run</strong> — the waveform and its spectrum are drawn here.</>}
        />
      </div>
    </>
  );
};

const FILTER_KINDS: Array<{ value: FilterKind; label: string }> = [
  { value: 'low', label: 'Low-pass' },
  { value: 'high', label: 'High-pass' },
  { value: 'band', label: 'Band-pass' },
  { value: 'stop', label: 'Band-stop' },
];

const FilterMode: React.FC<LiveSerialPlotPanelProps> = ({ theme, invoke }) => {
  const [order, setOrder] = useState(4);
  const [kind, setKind] = useState<FilterKind>('low');
  const [f1, setF1] = useState(100);
  const [f2, setF2] = useState(200);
  const [fs, setFs] = useState(1000);
  const { plots, error, running, run } = useRunOnce(invoke);
  const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const twoBand = kind === 'band' || kind === 'stop';
  const code = useMemo(() => filterScript(order, kind, f1, f2, fs), [order, kind, f1, f2, fs]);

  // Live-updates the frequency-response figure as controls change, debounced
  // so dragging a slider doesn't spawn a `qu` process per pixel.
  useEffect(() => {
    if (debounceRef.current) clearTimeout(debounceRef.current);
    debounceRef.current = setTimeout(() => void run(code), 350);
    return () => {
      if (debounceRef.current) clearTimeout(debounceRef.current);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [code]);

  const field = (label: string, control: React.ReactNode) => (
    <label className="flex flex-col gap-1 text-xs">
      <span style={{ color: 'var(--qu-muted)' }}>{label}</span>
      {control}
    </label>
  );
  const inputClass = 'px-2 py-1.5 rounded-md text-sm bg-transparent border';

  return (
    <>
      <div className="w-[22rem] flex-shrink-0 border-r overflow-y-auto flex flex-col gap-3 p-3" style={{ borderColor: border }}>
        <div className="flex items-center justify-between">
          <span className="text-sm font-medium">Filter design</span>
          <OnlineComingSoon />
        </div>
        {field(
          'Type',
          <select value={kind} onChange={(e) => setKind(e.target.value as FilterKind)} className={inputClass} style={{ borderColor: border }}>
            {FILTER_KINDS.map((k) => (
              <option key={k.value} value={k.value}>
                {k.label}
              </option>
            ))}
          </select>,
        )}
        {field(
          'Order',
          <input type="number" min={1} max={20} value={order} onChange={(e) => setOrder(Number(e.target.value))} className={inputClass} style={{ borderColor: border }} />,
        )}
        <div className="flex gap-2">
          {field(
            twoBand ? 'Cutoff f1 (Hz)' : 'Cutoff (Hz)',
            <input type="number" value={f1} onChange={(e) => setF1(Number(e.target.value))} className={inputClass} style={{ borderColor: border }} />,
          )}
          {twoBand &&
            field(
              'Cutoff f2 (Hz)',
              <input type="number" value={f2} onChange={(e) => setF2(Number(e.target.value))} className={inputClass} style={{ borderColor: border }} />,
            )}
        </div>
        {field(
          'Sample rate fs (Hz)',
          <input type="number" value={fs} onChange={(e) => setFs(Number(e.target.value))} className={inputClass} style={{ borderColor: border }} />,
        )}
        {error && (
          <div className="qu-banner" data-kind="error" style={{ borderRadius: 8, borderTopWidth: 1, borderStyle: 'solid', alignItems: 'flex-start', fontSize: 12.5, padding: '8px 10px' }}>
            <AlertCircle size={14} className="flex-shrink-0 mt-0.5" />
            <span className="break-words">{error}</span>
          </div>
        )}
        <div className="rounded-lg p-2 text-xs font-mono overflow-x-auto" style={{ background: 'var(--qu-code-bg)', border: '1px solid var(--qu-border)', color: 'var(--qu-text)' }}>
          {code}
        </div>
        <p className="text-xs" style={{ color: 'var(--qu-muted)' }}>
          {running ? 'Updating…' : "Updates live as you change a control -- filt.fvtool() draws magnitude/phase/group-delay together."}
        </p>
      </div>
      <div className="flex-1 min-w-0 overflow-auto p-3">
        <FigureViewer
          images={plots}
          theme={theme}
          emptyHint={<>Adjust a control — the response redraws here on its own.</>}
        />
      </div>
    </>
  );
};

export default DspWorkbenchPanel;
