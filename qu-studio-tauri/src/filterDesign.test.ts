import { describe, expect, it } from 'vitest';
import {
  FAMILIES,
  FILTER_KINDS,
  SIGNALS,
  buildFilterScript,
  isIdentifier,
  parseFilterNumbers,
  signalVariables,
  validateDesign,
  type DesignSpec,
} from './filterDesign';

const base: DesignSpec = {
  family: 'butter', kind: 'low', order: 4, f1: 100, f2: 200, fs: 1000, ripple: 1, atten: 40, signal: 'tones',
};

describe('validateDesign', () => {
  it('accepts a sensible design', () => {
    expect(validateDesign(base)).toBeNull();
  });

  it('rejects edges at or beyond Nyquist, zero, or reversed', () => {
    expect(validateDesign({ ...base, f1: 500 })).toMatch(/Nyquist/);
    expect(validateDesign({ ...base, f1: 0 })).toMatch(/above 0/);
    expect(validateDesign({ ...base, kind: 'band', f1: 300, f2: 100 })).toMatch(/below edge f2/);
    expect(validateDesign({ ...base, kind: 'band', f1: 100, f2: 600 })).toMatch(/Nyquist/);
  });

  it('rejects bad orders and sample rates', () => {
    expect(validateDesign({ ...base, order: 2.5 })).toMatch(/whole number/);
    expect(validateDesign({ ...base, order: 0 })).toMatch(/whole number/);
    expect(validateDesign({ ...base, order: 300 })).toMatch(/200/);
    expect(validateDesign({ ...base, fs: 0 })).toMatch(/Sample rate/);
  });

  it('needs ripple and attenuation only for the families that use them', () => {
    expect(validateDesign({ ...base, family: 'cheby1', ripple: 0 })).toMatch(/ripple/);
    expect(validateDesign({ ...base, family: 'cheby2', atten: 0 })).toMatch(/attenuation/);
    expect(validateDesign({ ...base, family: 'ellip', ripple: 1, atten: 0 })).toMatch(/attenuation/);
    expect(validateDesign({ ...base, family: 'butter', ripple: 0, atten: 0 })).toBeNull();
  });

  it('refuses an odd-order high-pass or band-stop FIR (zero at Nyquist)', () => {
    expect(validateDesign({ ...base, family: 'fir1', kind: 'high', order: 5 })).toMatch(/even order/);
    expect(validateDesign({ ...base, family: 'fir1', kind: 'stop', f1: 100, f2: 200, order: 5 })).toMatch(/even order/);
    expect(validateDesign({ ...base, family: 'fir1', kind: 'low', order: 5 })).toBeNull();
    expect(validateDesign({ ...base, family: 'fir1', kind: 'high', order: 6 })).toBeNull();
  });
});

describe('buildFilterScript', () => {
  it('writes the design line for every family', () => {
    const line = (family: DesignSpec['family']) => buildFilterScript({ ...base, family }, false).split('\n')[1];
    expect(line('butter')).toBe('filt = butter(4, "low", 100, fs)');
    expect(line('cheby1')).toBe('filt = cheby1(4, 1, "low", 100, fs)');
    expect(line('cheby2')).toBe('filt = cheby2(4, 40, "low", 100, fs)');
    expect(line('ellip')).toBe('filt = ellip(4, 1, 40, "low", 100, fs)');
    expect(line('fir1')).toBe('filt = fir1(4, 100, kind="low", fs=fs)');
  });

  it('writes a [low, high] cutoff for the two-edge types', () => {
    const s = buildFilterScript({ ...base, kind: 'band' }, false);
    expect(s).toContain('filt = butter(4, "band", [100, 200], fs)');
  });

  it('the shown script is short and no longer draws with fvtool', () => {
    const shown = buildFilterScript(base, false);
    expect(shown).toBe('fs = 1000\nfilt = butter(4, "low", 100, fs)\ninfo = filter_info(filt)\n');
    expect(shown).not.toContain('fvtool');
  });

  it('the full script binds every vector the views read', () => {
    const full = buildFilterScript(base, true);
    for (const name of ['f', 'mag_db', 'ph', 'hr', 'hi', 'gd', 'h', 's', 'pr', 'pim', 'zr', 'zim', 'x', 'sig', 'y', 'sx', 'sy', 'sf', 'fi']) {
      expect(full, name).toMatch(new RegExp(`^${name} = `, 'm'));
    }
  });

  it('every signal source generates a sig for every example signal', () => {
    for (const { id } of SIGNALS) {
      expect(buildFilterScript({ ...base, signal: id }, true), id).toMatch(/^sig = /m);
    }
  });

  it('with your own variable the script reads the exported samples and filters the variable', () => {
    const spec: DesignSpec = { ...base, source: 'variable', varName: 'mysig' };
    const full = buildFilterScript(spec, true, 'C:\\cache\\sig.f64');
    expect(full).toContain('sig = read_array("C:/cache/sig.f64")');
    expect(full).not.toContain('x = 0 to 0.999');
    const shown = buildFilterScript(spec, false);
    expect(shown).toContain('sig = mysig\ny = sosfilt(filt, sig)\n');
  });

  it('ignores a variable name that is not a plain identifier', () => {
    const spec: DesignSpec = { ...base, source: 'variable', varName: 'a; b' };
    expect(buildFilterScript(spec, false)).not.toContain('a; b');
  });
});

describe('parseFilterNumbers', () => {
  const fi = [0.005, 5, 2.5, 18, 100, null, 100, null, 0.0045, 0.0044, 0.004, 0.016, 11.9, 0, 0.2, 1];

  it('reads the fixed order of the vector', () => {
    const n = parseFilterNumbers(fi)!;
    expect(n.delay).toBe(0.005);
    expect(n.impulseLength).toBe(18);
    expect(n.cutoff).toBe(100);
    expect(n.edgeLow).toBeNull(); // a low-pass has no lower edge
    expect(n.tau).toBe(0.0045);
    expect(n.overshoot).toBe(11.9);
    expect(n.stable).toBe(true);
  });

  it('reports unstable when the flag is 0', () => {
    expect(parseFilterNumbers([...fi.slice(0, 15), 0])!.stable).toBe(false);
  });

  it('returns null for a missing or short vector', () => {
    expect(parseFilterNumbers(undefined)).toBeNull();
    expect(parseFilterNumbers([1, 2, 3])).toBeNull();
  });
});

describe('signalVariables', () => {
  it('keeps real 1-D numeric vectors of at least 8 samples', () => {
    const out = signalVariables([
      { name: 'a', shape: [100, 1] },
      { name: 'b', shape: [1, 50] },
      { name: 'tiny', shape: [4, 1] },
      { name: 'mat', shape: [10, 10] },
      { name: 'z', type: 'complex vector', shape: [100, 1] },
      { name: 'big', shape: [5_000_000, 1], length: 5_000_000 },
    ]);
    expect(out.map((v) => v.name)).toEqual(['a', 'b', 'big']);
    expect(out[2].length).toBe(5_000_000);
  });
});

describe('isIdentifier', () => {
  it('accepts names and record fields only', () => {
    expect(isIdentifier('sig')).toBe(true);
    expect(isIdentifier('res.field')).toBe(true);
    expect(isIdentifier('a b')).toBe(false);
    expect(isIdentifier('1x')).toBe(false);
    expect(isIdentifier('x; y')).toBe(false);
    expect(isIdentifier(undefined)).toBe(false);
  });
});

describe('tables', () => {
  it('every family offers every filter type', () => {
    expect(FAMILIES.length).toBe(5);
    expect(FILTER_KINDS.map((k) => k.id)).toEqual(['low', 'high', 'band', 'stop']);
  });
});
