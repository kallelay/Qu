"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const { fftReal, makeRange, runSource, tokenize } = require("../docs/assets/qu-worker.js");

test("tokenizer recognizes numerical units and range words", () => {
  const tokens = tokenize("t = 0 s to 10 ms step 1 ms");
  assert.deepEqual(
    tokens.slice(0, -1).map(token => token.value),
    ["t", "=", 0, "s", "to", 10, "ms", "step", 1, "ms"]
  );
});

test("inclusive range has predictable numerical length", () => {
  const range = makeRange(0, 1, 0.25);
  assert.deepEqual(Array.from(range), [0, 0.25, 0.5, 0.75, 1]);
});

test("browser syntax evaluates mixed exponentiation precedence like native Qu", () => {
  const tokens = tokenize("answer = (1*3**4-1+(2*4))");
  assert.ok(tokens.some(token => token.value === "**"));
  const result = runSource("answer = (1*3**4-1+(2*4))\nprint answer");
  assert.deepEqual(result.diagnostics, []);
  assert.deepEqual(result.logs, ["88"]);
});

test("browser range parity keeps a long decimal inclusive stop", () => {
  const range = makeRange(1, 20, 0.01);
  assert.equal(range.length, 1901);
  assert.ok(Math.abs(range[range.length - 1] - 20) < 1e-12);
});

test("radix-2 FFT transforms an impulse", () => {
  const spectrum = fftReal(Float64Array.from([1, 0, 0, 0, 0, 0, 0, 0]));
  assert.deepEqual(Array.from(spectrum.re), new Array(8).fill(1));
  assert.ok(Array.from(spectrum.im).every(value => Math.abs(value) < 1e-12));
});

test("multisine vertical slice executes, reports, and plots", () => {
  const source = `Fs = 5 kHz
N = 2048
t = 0 s to (N - 1)/Fs step 1/Fs
x = sin(2*pi*50*t) + 0.3*sin(2*pi*420*t)
x -= mean(x)
X = x.fft().abs()

plot(t, x)
X.plot(logy=on)
print "samples = {numel(x)}; crest = {max(abs(x))/rms(x):.2f}"`;

  const result = runSource(source);
  assert.deepEqual(result.diagnostics, []);
  assert.equal(result.plots.length, 2);
  assert.equal(result.plots[0].y.length, 2048);
  assert.equal(result.plots[1].y.length, 1024);
  assert.match(result.logs[0], /^samples = 2048; crest = \d+\.\d{2}$/);
  assert.ok(result.bindings.some(binding => binding.name === "X" && binding.summary === "vector[2048]"));
});

test("unknown functions become line-addressed Qu diagnostics", () => {
  const result = runSource("x = 1\ny = mystery(x)\nz = 3");
  assert.equal(result.diagnostics.length, 1);
  assert.deepEqual(
    { code: result.diagnostics[0].code, line: result.diagnostics[0].line },
    { code: "Q1001", line: 2 }
  );
  assert.match(result.diagnostics[0].message, /mystery/);
  assert.ok(!result.bindings.some(binding => binding.name === "z"));
});

test("logical assignment, where indices, gathering, and plot markers compose", () => {
  const source = `x = -2 to 4 step 1
x[x < 0] = 0;
idx1 := where x > 3
idx2 := where x > 2
x.plot()
plot(x[idx1], 'o', color='red')
plot(x[idx2], 'x', color='blue')
show()`;
  const result = runSource(source);
  assert.deepEqual(result.diagnostics, []);
  assert.equal(result.plots.length, 3);
  assert.deepEqual(result.plots[0].y, [0, 0, 0, 1, 2, 3, 4]);
  assert.deepEqual(result.plots[1].y, [4]);
  assert.deepEqual(result.plots[2].y, [3, 4]);
  assert.deepEqual(result.plots[1].x, [6]);
  assert.deepEqual(result.plots[2].x, [5, 6]);
  assert.deepEqual(
    result.plots.slice(1).map(plot => [plot.marker, plot.color]),
    [["o", "red"], ["x", "blue"]]
  );
});

test("randn dimensions preserve matrix shape and empty where results are valid", () => {
  const result = runSource(`x = randn(3, 1)\nx[x < 0] = 0\nidx := where x > 99\nplot(x[idx], 'o')\nshow()`);
  assert.deepEqual(result.diagnostics, []);
  assert.ok(result.bindings.some(binding => binding.name === "x" && binding.summary === "matrix[3x1]"));
  assert.ok(result.bindings.some(binding => binding.name === "idx" && binding.summary === "vector[0]"));
  assert.deepEqual(result.plots[0].y, []);
});
