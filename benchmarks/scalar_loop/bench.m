% Sustained scalar iteration -- MATLAB side.
%
% Same biquad, same coefficients, same input, same order as bench.qu beside
% this file. See bench.qu's header for why this scenario exists.
%
% Written as an explicit loop on purpose. MATLAB's own `filter()` would
% measure its compiled DSP routine, not the language; the language is what
% the M4 JIT decision turns on. This is also the shape MATLAB's JIT is
% specifically good at, so it is a fair place for MATLAB to look strong.

N = 1000000;

b0 = 0.0675; b1 = 0.1349; b2 = 0.0675;
a1 = -1.1430; a2 = 0.4128;

% Input built outside the clock, no RNG, identical to the other two ports.
x = zeros(N, 1);
for i = 1:N
    k = i - 1;                     % 0-based index, to match bench.qu/.py
    x(i) = sin(k * 0.0001) + 0.5 * sin(k * 0.0013);
end

tic;
y = zeros(N, 1);
w1 = 0.0;
w2 = 0.0;
for i = 1:N
    w0 = x(i) - a1 * w1 - a2 * w2;
    y(i) = b0 * w0 + b1 * w1 + b2 * w2;
    w2 = w1;
    w1 = w0;
end
t_iir = toc;

fprintf('biquad, %d samples, scalar loop : %.4f s\n', N, t_iir);
fprintf('checksum                          : %.6f\n', sum(y));
fprintf('per-iteration                     : %.1f ns\n', t_iir / N * 1e9);
