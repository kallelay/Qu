/** Pure helpers behind the filter designer: the Qu script it generates, the
 *  checks that keep an invalid design from ever reaching the engine, and the
 *  reading of the numbers vector the script ends with. No DOM, no Tauri, so
 *  the logic is testable on its own. */

export type FilterKind = 'low' | 'high' | 'band' | 'stop';
export type FilterFamily = 'butter' | 'cheby1' | 'cheby2' | 'ellip' | 'fir1';
export type SignalId = 'tones' | 'noisy' | 'chirp' | 'step' | 'impulse';
/** Where the filtered signal comes from: a generated example, or a numeric
 *  vector from the Code tab's session (the last run's variables). */
export type SignalSource = 'example' | 'variable';

export interface DesignSpec {
  family: FilterFamily;
  kind: FilterKind;
  order: number;
  f1: number;
  f2: number;
  fs: number;
  ripple: number;
  atten: number;
  signal: SignalId;
  source?: SignalSource;
  /** With `source: 'variable'`: the session variable's name. */
  varName?: string;
}

/** A session variable usable as the designer's input: a real, 1-D numeric
 *  vector long enough to filter. */
export interface SignalVariable {
  name: string;
  length: number;
}

/** The numeric variables of a run that the designer can filter: real
 *  vectors (one row or one column) of at least 8 samples. */
export function signalVariables(
  vars: Array<{ name: string; type?: string; shape?: number[]; length?: number; data?: unknown[] }>,
): SignalVariable[] {
  const out: SignalVariable[] = [];
  for (const v of vars) {
    if (/complex/i.test(v.type ?? '')) continue;
    const shape = v.shape ?? [];
    if (shape.length > 2 || (shape.length === 2 && Math.min(shape[0], shape[1]) !== 1)) continue;
    const n = v.length ?? (shape.length === 2 ? shape[0] * shape[1] : v.data?.length ?? 0);
    if (n >= 8) out.push({ name: v.name, length: n });
  }
  return out;
}

/** True for a name Qu reads as a variable: `sig`, or a record field the
 *  Variables panel lists as `res.field`. */
export const isIdentifier = (name: string | undefined): name is string =>
  !!name && /^[A-Za-z_][A-Za-z0-9_]*(\.[A-Za-z_][A-Za-z0-9_]*)?$/.test(name);

export const FAMILIES: Array<{ id: FilterFamily; label: string; ripple: boolean; atten: boolean; fir: boolean }> = [
  { id: 'butter', label: 'Butterworth (flat)', ripple: false, atten: false, fir: false },
  { id: 'cheby1', label: 'Chebyshev I (passband ripple)', ripple: true, atten: false, fir: false },
  { id: 'cheby2', label: 'Chebyshev II (stopband ripple)', ripple: false, atten: true, fir: false },
  { id: 'ellip', label: 'Elliptic (steepest)', ripple: true, atten: true, fir: false },
  { id: 'fir1', label: 'FIR (linear phase)', ripple: false, atten: false, fir: true },
];

export const FILTER_KINDS: Array<{ id: FilterKind; label: string }> = [
  { id: 'low', label: 'Low-pass' },
  { id: 'high', label: 'High-pass' },
  { id: 'band', label: 'Band-pass' },
  { id: 'stop', label: 'Band-stop' },
];

export const SIGNALS: Array<{ id: SignalId; label: string }> = [
  { id: 'tones', label: 'Two tones (low + high)' },
  { id: 'noisy', label: 'Sine in noise' },
  { id: 'chirp', label: 'Chirp (sweep)' },
  { id: 'step', label: 'Step' },
  { id: 'impulse', label: 'Impulse' },
];

const isBand = (k: FilterKind) => k === 'band' || k === 'stop';

/** `null` when the design is fine, else one sentence saying what to change. */
export function validateDesign(s: DesignSpec): string | null {
  if (!(s.fs > 0)) return 'Sample rate must be above 0 Hz.';
  const nyq = s.fs / 2;
  if (!Number.isInteger(s.order) || s.order < 1) return 'Order must be a whole number, 1 or more.';
  if (s.order > 200) return 'Order above 200 is not offered here.';
  if (!(s.f1 > 0) || s.f1 >= nyq) return `The cutoff must be above 0 and below Nyquist (${nyq} Hz).`;
  if (isBand(s.kind)) {
    if (!(s.f2 > 0) || s.f2 >= nyq) return `Edge f2 must be above 0 and below Nyquist (${nyq} Hz).`;
    if (s.f1 >= s.f2) return 'Edge f1 must be below edge f2.';
  }
  const fam = FAMILIES.find((f) => f.id === s.family)!;
  if (fam.ripple && !(s.ripple > 0)) return 'Passband ripple must be above 0 dB.';
  if (fam.atten && !(s.atten > 0)) return 'Stopband attenuation must be above 0 dB.';
  if (s.source === 'variable' && !isIdentifier(s.varName)) {
    return 'Pick a variable from the last run (run a script in the Code tab first).';
  }
  if (fam.fir && s.order % 2 === 1 && (s.kind === 'high' || s.kind === 'stop')) {
    return 'A high-pass or band-stop FIR needs an even order.';
  }
  return null;
}

function designLine(s: DesignSpec): string {
  const cutoff = isBand(s.kind) ? `[${s.f1}, ${s.f2}]` : `${s.f1}`;
  switch (s.family) {
    case 'butter':
      return `filt = butter(${s.order}, "${s.kind}", ${cutoff}, fs)`;
    case 'cheby1':
      return `filt = cheby1(${s.order}, ${s.ripple}, "${s.kind}", ${cutoff}, fs)`;
    case 'cheby2':
      return `filt = cheby2(${s.order}, ${s.atten}, "${s.kind}", ${cutoff}, fs)`;
    case 'ellip':
      return `filt = ellip(${s.order}, ${s.ripple}, ${s.atten}, "${s.kind}", ${cutoff}, fs)`;
    case 'fir1':
      return `filt = fir1(${s.order}, ${cutoff}, kind="${s.kind}", fs=fs)`;
  }
}

function signalLines(id: SignalId, fs: number): string {
  const lo = Math.round(fs * 0.05 * 100) / 100;
  const hi = Math.round(fs * 0.3 * 100) / 100;
  switch (id) {
    case 'tones':
      return `sig = sin(2 * pi * ${lo} * x) + 0.5 * sin(2 * pi * ${hi} * x)`;
    case 'noisy':
      return `sig = sin(2 * pi * ${lo} * x) + 0.5 * randn(length(x))`;
    case 'chirp':
      return `sig = sin(2 * pi * (${Math.round(fs * 0.01)} * x + ${Math.round(fs * 0.2)} * x * x))`;
    case 'step':
      return 'sig = (x >= 0.1) * 1.0';
    case 'impulse':
      return 'sig = 0 * x\nsig[0] = 1';
  }
}

/** The Qu program. With `full = false` only the part worth showing, copying
 *  and opening in the editor (the design, a one-line check, and the filtering
 *  of the user's own variable when that is the source); with `full = true`
 *  the hidden footer that binds every vector the views draw.
 *
 *  `signalFile`: with `source: 'variable'` the hidden run cannot see the
 *  Code tab's session (it is a separate process), so the variable's samples
 *  are handed over as a raw little-endian f64 file the script reads back. */
export function buildFilterScript(s: DesignSpec, full = true, signalFile = ''): string {
  const head = `fs = ${s.fs}\n${designLine(s)}\n`;
  const own = s.source === 'variable' && isIdentifier(s.varName);
  if (!full) {
    const tail = own ? `sig = ${s.varName}\ny = sosfilt(filt, sig)\n` : '';
    return `${head}info = filter_info(filt)\n${tail}`;
  }
  const signal = own
    ? `sig = read_array("${signalFile.split('\\').join('/')}")\nx = (0 to length(sig) - 1) / fs`
    : `x = 0 to 0.999 step 1 / fs\n${signalLines(s.signal, s.fs)}`;
  return `${head}${signal}
y = sosfilt(filt, sig)
H = freqz(filt, 512)
f = (0 to 511) * (fs / 2) / 511
mag_db = 20 * log10(abs(H) + 1e-12)
ph = angle(H)
hr = real(H)
hi = imag(H)
gd = group_delay(filt, 512)
h = impz(filt)
s = stepz(filt)
p = poles(filt)
z = zeros(filt)
pr = real(p)
pim = imag(p)
zr = real(z)
zim = imag(z)
sx = abs(rfft(sig))
sy = abs(rfft(y))
sf = (0 to length(sx) - 1) * fs / length(sig)
info = filter_info(filt)
fi = [info.delay, info.delay_samples, info.delay_spread_samples, info.impulse_length, info.cutoff_3db, info.edge_low, info.edge_high, info.bandwidth, info.tau, info.tau_pole, info.rise_time, info.settling_time, info.overshoot_pct, info.dc_gain_db, info.stability_margin, info.stable * 1.0]
`;
}

export interface FilterNumbers {
  delay: number;
  delaySamples: number;
  delaySpread: number;
  impulseLength: number;
  cutoff: number | null;
  edgeLow: number | null;
  edgeHigh: number | null;
  bandwidth: number | null;
  tau: number | null;
  tauPole: number | null;
  rise: number | null;
  settle: number | null;
  overshoot: number | null;
  dcGainDb: number | null;
  stabilityMargin: number | null;
  stable: boolean;
}

/** The `fi` vector the script ends with, in its fixed order. `null` for a
 *  value the engine could not measure (a NaN arrives as null). */
export function parseFilterNumbers(fi: (number | null)[] | undefined): FilterNumbers | null {
  if (!fi || fi.length < 16) return null;
  const v = (i: number) => fi[i];
  const n = (i: number) => (v(i) == null ? NaN : (v(i) as number));
  // Step-response measures are relative to the step's final value, which is
  // the DC gain. A filter that blocks DC (high-pass, band-pass) settles at
  // ~0, so "63 % of final", "10-90 % rise" and "% overshoot" divide by
  // nothing: the engine still returns numbers (0 ms, 2011 %), which read as
  // measurements. Below -20 dB of DC gain they are not shown.
  const dc = v(13);
  const blocksDc = dc == null || dc < -20;
  const step = (i: number) => (blocksDc ? null : v(i));
  return {
    delay: n(0),
    delaySamples: n(1),
    delaySpread: n(2),
    impulseLength: n(3),
    cutoff: v(4),
    edgeLow: v(5),
    edgeHigh: v(6),
    bandwidth: v(7),
    tau: step(8),
    tauPole: v(9),
    rise: step(10),
    settle: step(11),
    overshoot: step(12),
    dcGainDb: v(13),
    stabilityMargin: v(14),
    stable: v(15) === 1,
  };
}
