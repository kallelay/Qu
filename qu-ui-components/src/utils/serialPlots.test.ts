import { describe, expect, it } from 'vitest';
import { parseIndexFilter, serialFigures } from './serialPlots';

describe('SeriPlot measurement views', () => {
  it('filters exact intervals and exclusions without expanding them', () => {
    const keep = parseIndexFilter('0-5,8,10-19');
    expect([0, 5, 6, 8, 9, 19, 20].filter(keep)).toEqual([0, 5, 8, 19]);
    expect([0, 3, 7, 8].filter(parseIndexFilter('skip:3,7'))).toEqual([0, 8]);
    expect(parseIndexFilter('0-9007199254740991')(3)).toBe(true);
    for (const invalid of ['skip:', '3-1', '-2', '1.5', '1,', 'NaN', '9007199254740992']) expect(() => parseIndexFilter(invalid)).toThrow();
  });
  it('converts degree phase to Nyquist with equal axes and correct sign', () => {
    const frames = [{ id: 1, rows: [[0, 10, -90, 20, -45], [1, 5, 0, 6, 0]] }];
    const plots = serialFigures('impedance', 'nyquist-bode', frames, false, parseIndexFilter('0'));
    expect(plots).toHaveLength(3);
    expect(plots[0].equalAspect).toBe(true);
    expect(plots[0].data[0].x![0]).toBeCloseTo(0);
    expect(plots[0].data[0].y).toEqual([10]);
    expect(plots[1].data[1].y).toEqual([20]);
    expect(plots[2].data[1].y).toEqual([-45]);
    expect(plots[1].xlabel).toBe('Frequency index');
    expect(frames[0].rows).toHaveLength(2);
  });
  it('bounds fading at ten, uses only the latest legend, and preserves both ADC channels', () => {
    const frames = Array.from({ length: 15 }, (_, id) => ({ id, rows: [[id, -id], [id + 1, 2]] }));
    const figures = serialFigures('signal', 'nyquist', frames, true, () => false);
    expect(figures).toHaveLength(2);
    expect(figures[0].data).toHaveLength(10);
    expect(figures[0].data[0].opacity).toBe(0.15);
    expect(figures[0].data[9].opacity).toBe(1);
    expect(figures[0].data.filter(trace => trace.showlegend)).toHaveLength(1);
    expect(figures[1].data[9].y).toEqual([-14, 2]);
    expect(figures[1].data[9].x).toEqual([0, 1]);
    expect(serialFigures('signal', 'nyquist', frames, false, () => true)[0].data).toHaveLength(1);
  });
});
