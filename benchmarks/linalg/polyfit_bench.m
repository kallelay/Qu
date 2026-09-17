% polyfit_bench.m -- MATLAB port of polyfit_bench.qu/.py, bit-identical
% (formula-generated, no RNG) dataset so the fitted coefficients can be
% compared directly across languages, not just the timings.
%
% Also runnable unmodified under matty's own interpreter:
%   python matty_runner.py <path to this file>   (from the matty/ repo root)
%
% Run: matlab -batch "run('benchmarks/linalg/polyfit_bench.m')"

N = 3000000;
degree = 5;
true_coeffs = [2, -4, 1, 0.5, -2, 3];   % highest-degree-first

x = linspace(-1, 1, N);
y_true = polyval(true_coeffs, x);
noise = 0.01 * sin(1000 * x);
y = y_true + noise;

tic; c = polyfit(x, y, degree); t_fit = toc;
tic; yhat = polyval(c, x); t_eval = toc;

resid = norm(y - yhat);
max_abs_err = max(abs(y - yhat));

fprintf('N=%d  degree=%d\n', N, degree);
fprintf('polyfit : %.4f s\n', t_fit);
fprintf('polyval : %.4f s\n', t_eval);
fprintf('coeffs  : ');
fprintf('%.6f ', c);
fprintf('\n');
fprintf('resid (||y - yhat||_2) = %.6f\n', resid);
fprintf('max_abs_err            = %.6f\n', max_abs_err);
