import { describe, it, expect } from 'vitest';
import { readFigureGeometry, panelAt, dataAt, pixelAt, formatCoord, axisResolution, formatSnapped } from './figureGeometry';

// The exact shape the engine emits, escaped the way it escapes it.
const META = `{&quot;version&quot;:1,&quot;width&quot;:900.0000,&quot;height&quot;:600.0000,&quot;panels&quot;:[{&quot;panel&quot;:0,&quot;left&quot;:86.0000,&quot;right&quot;:866.0000,&quot;top&quot;:48.0000,&quot;bottom&quot;:540.0000,&quot;xmin&quot;:0.0000,&quot;xmax&quot;:10.0000,&quot;ymin&quot;:-1.0000,&quot;ymax&quot;:1.0000,&quot;logX&quot;:false,&quot;logY&quot;:false,&quot;xBreak&quot;:null,&quot;yBreak&quot;:null}]}`;
const SVG = `<svg xmlns="http://www.w3.org/2000/svg">\n<metadata id="qu-figure">${META}</metadata>\n<rect/>\n</svg>`;

describe('readFigureGeometry', () => {
  it('reads the mapping out of a rendered figure', () => {
    const geo = readFigureGeometry(SVG);
    expect(geo).not.toBeNull();
    expect(geo!.panels).toHaveLength(1);
    expect(geo!.panels[0].left).toBe(86);
    expect(geo!.panels[0].xmax).toBe(10);
  });

  it('returns null rather than throwing for a figure without metadata', () => {
    // An older engine's output, or a raster image. The viewer must degrade
    // to "no readout", not crash.
    expect(readFigureGeometry('<svg><rect/></svg>')).toBeNull();
    expect(readFigureGeometry(null)).toBeNull();
    expect(readFigureGeometry('<svg><metadata id="qu-figure">not json</metadata></svg>')).toBeNull();
  });
});

describe('dataAt / pixelAt', () => {
  const p = readFigureGeometry(SVG)!.panels[0];

  it('maps the plot corners to the axis limits', () => {
    expect(dataAt(p, 86, 540)).toEqual({ x: 0, y: -1 });   // bottom-left
    expect(dataAt(p, 866, 48)).toEqual({ x: 10, y: 1 });    // top-right
  });

  it('maps the centre to the middle of both ranges', () => {
    const mid = dataAt(p, (86 + 866) / 2, (48 + 540) / 2);
    expect(mid.x).toBeCloseTo(5, 9);
    expect(mid.y).toBeCloseTo(0, 9);
  });

  it('round-trips data -> pixel -> data', () => {
    for (const [x, y] of [[0, -1], [2.5, 0.3], [7.75, -0.62], [10, 1]]) {
      const px = pixelAt(p, x, y);
      const back = dataAt(p, px.x, px.y);
      expect(back.x).toBeCloseTo(x, 9);
      expect(back.y).toBeCloseTo(y, 9);
    }
  });

  it('inverts a logarithmic axis through the stored log limits', () => {
    // Limits are stored post-log10, so 0..3 means 1..1000.
    const log = { ...p, logY: true, ymin: 0, ymax: 3 };
    expect(dataAt(log, 86, 540).y).toBeCloseTo(1, 9);
    expect(dataAt(log, 86, 48).y).toBeCloseTo(1000, 9);
    const mid = dataAt(log, 86, (48 + 540) / 2);
    expect(mid.y).toBeCloseTo(10 ** 1.5, 6);
    // ...and back again.
    expect(pixelAt(log, 5, 1000).y).toBeCloseTo(48, 6);
    expect(pixelAt(log, 5, 1).y).toBeCloseTo(540, 6);
  });
});

describe('panelAt', () => {
  const geo = readFigureGeometry(SVG)!;
  it('finds the panel under a point and nothing outside it', () => {
    expect(panelAt(geo, 400, 300)?.panel).toBe(0);
    expect(panelAt(geo, 10, 10)).toBeNull();    // in the margin
    expect(panelAt(geo, 400, 580)).toBeNull();  // below the plot area
  });
});

describe('formatCoord', () => {
  it('keeps ordinary numbers readable and switches to exponent at the extremes', () => {
    expect(formatCoord(0)).toBe('0');
    expect(formatCoord(1.5)).toBe('1.5');
    expect(formatCoord(1 / 3)).toBe('0.333333');
    expect(formatCoord(0.00001)).toBe('1.000e-5');
    expect(formatCoord(12345678)).toBe('1.235e+7');
    expect(formatCoord(NaN)).toBe('—');
  });
});

describe('formatSnapped', () => {
  const p = readFigureGeometry(SVG)!.panels[0];

  it('reports a point that is exactly 9 as 9, not 9.00005', () => {
    // The bug this exists for: the renderer writes coordinates with two
    // decimals, so inverting a drawn vertex lands a hair off the true
    // value. Printing six significant figures turns that round-trip error
    // into false precision on screen.
    const res = axisResolution(p, 'x');
    expect(formatSnapped(9.00005, res)).toBe('9');
    expect(formatSnapped(4.99997, res)).toBe('5');
  });

  it('still distinguishes values the figure can actually resolve', () => {
    const res = axisResolution(p, 'x');
    // x spans 10 over 780px, so ~1.3e-4 per 0.01px: 9.01 and 9.02 are
    // genuinely different positions and must not collapse together.
    expect(formatSnapped(9.01, res)).not.toBe(formatSnapped(9.02, res));
  });

  it('falls back to the ordinary format for a nonsense resolution', () => {
    expect(formatSnapped(1 / 3, 0)).toBe(formatCoord(1 / 3));
    expect(formatSnapped(NaN, 1)).toBe('—');
  });
});
