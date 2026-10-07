import { describe, it, expect } from 'vitest';
import { fitScale, fitView, zoomAt, clampZoom, intrinsicSize, wheelFactor, MAX_ZOOM } from './viewTransform';

describe('viewTransform', () => {
  it('fits a wide drawing by width and centres it', () => {
    const v = fitView(1000, 600, 2000, 500, 0);
    expect(v.k).toBeCloseTo(0.5);
    expect(v.x).toBeCloseTo(0);
    expect(v.y).toBeCloseTo((600 - 250) / 2);
  });
  it('can refuse to upscale', () => {
    expect(fitScale(1000, 1000, 10, 10, 0, false)).toBe(1);
    expect(fitScale(1000, 1000, 10, 10, 0, true)).toBeGreaterThan(1);
  });
  it('zoomAt keeps the point under the cursor fixed', () => {
    const v0 = { k: 1, x: 30, y: 40 };
    const v1 = zoomAt(v0, 2, 200, 100);
    // content point under cursor before and after
    expect((200 - v0.x) / v0.k).toBeCloseTo((200 - v1.x) / v1.k);
    expect((100 - v0.y) / v0.k).toBeCloseTo((100 - v1.y) / v1.k);
  });
  it('clamps zoom and survives garbage', () => {
    expect(clampZoom(1e9)).toBe(MAX_ZOOM);
    expect(clampZoom(NaN)).toBe(1);
    expect(fitScale(0, 0, 0, 0)).toBe(1);
  });
  it('wheel up zooms in, down zooms out', () => {
    expect(wheelFactor(-100)).toBeGreaterThan(1);
    expect(wheelFactor(100)).toBeLessThan(1);
  });
  it('reads intrinsic size from width/height, pt, or viewBox', () => {
    expect(intrinsicSize({ width: '640', height: '480' })).toEqual({ w: 640, h: 480 });
    expect(intrinsicSize({ width: '72pt', height: '36pt' })).toEqual({ w: 96, h: 48 });
    expect(intrinsicSize({ viewBox: '0 0 100 50' })).toEqual({ w: 100, h: 50 });
    expect(intrinsicSize({ width: '200', viewBox: '0 0 100 50' })).toEqual({ w: 200, h: 100 });
    expect(intrinsicSize({ width: '100%', viewBox: '0 0 10 10' })).toEqual({ w: 10, h: 10 });
    expect(intrinsicSize({})).toEqual({ w: 300, h: 150 });
  });
});
