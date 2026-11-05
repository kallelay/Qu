/** Pure helpers behind the filter designer: the Qu script it generates, the
 *  checks that keep an invalid design from ever reaching the engine, and the
 *  reading of the numbers vector the script ends with. No DOM, no Tauri, so
 *  the logic is testable on its own. */

export type FilterKind = 'low' | 'high' | 'band' | 'stop';
export type FilterFamily = 'butter' | 'cheby1' | 'cheby2' | 'ellip' | 'fir1';
export type SignalId = 'tones' | 'noisy' | 'chirp' | 'step' | 'impulse';

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
}

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

/** The Qu program. With `full = false` only the part worth showing and
 *  copying (the design and a one-line check); with `full = true` the hidden
 *  footer that binds every vector the views draw. */
export function buildFilterScript(s: DesignSpec, full = true): string {
  const head = `fs = ${s.fs}\n${designLine(s)}\n`;
  if (!full) return `${head}info = filter_info(filt)\nfilt.fvtool()\n`;
  return `${head}x = 0 to 0.999 step 1 / fs
${signalLines(s.signal, s.fs)}
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
  return {
    delay: n(0),
    delaySamples: n(1),
    delaySpread: n(2),
    impulseLength: n(3),
    cutoff: v(4),
    edgeLow: v(5),
    edgeHigh: v(6),
    bandwidth: v(7),
    tau: v(8),
    tauPole: v(9),
    rise: v(10),
    settle: v(11),
    overshoot: v(12),
    dcGainDb: v(13),
    stabilityMargin: v(14),
    stable: v(15) === 1,
  };
}
