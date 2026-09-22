% bench_fair.m -- MATLAB port of bench_fair.qu.
%
% Same five kernels as bench.m, but with data built OUTSIDE the timed
% region, one discarded warm-up run, and best-of-three reported. This
% matters most for MATLAB: its first call to a library-backed kernel is
% warm-up dominated (fft measured at 0.0237 s cold and 0.0007 s on the
% third call, 34x), so bench.m's single cold trial per kernel reports
% MATLAB's start-up rather than its throughput.

reps = 3;

% matmul -- data pre-built
A = randn(1000); B = randn(1000);
C = A * B;                                  %#ok<NASGU> warm-up, discarded
best_mm = inf;
for r = 1:reps
    tic; C = A * B; t = toc;                %#ok<NASGU>
    best_mm = min(best_mm, t);
end
fprintf('matrix_multiply   : %.4f s   (data pre-built, best of %d)\n', best_mm, reps);

% elementwise -- 3000x3000 to match bench_fair.qu
A2 = randn(3000); B2 = randn(3000);
C2 = A2 .* B2 + sin(A2) .* cos(B2);         %#ok<NASGU> warm-up, discarded
best_ew = inf;
for r = 1:reps
    tic; C2 = A2 .* B2 + sin(A2) .* cos(B2); D2 = abs(C2); t = toc;  %#ok<NASGU>
    best_ew = min(best_ew, t);
end
fprintf('element_wise_ops  : %.4f s   (3000x3000, data pre-built, best of %d)\n', best_ew, reps);

% fft -- data pre-built
x = randn(100000, 1);
X = fft(x);                                 %#ok<NASGU> warm-up, discarded
best_fft = inf;
for r = 1:reps
    tic; X = fft(x); xr = ifft(X); t = toc; %#ok<NASGU>
    best_fft = min(best_fft, t);
end
fprintf('fft               : %.4f s   (data pre-built, best of %d)\n', best_fft, reps);

% array creation -- no rand, matching bench_fair.qu
Az = zeros(5000);                           %#ok<NASGU> warm-up, discarded
best_ac = inf;
for r = 1:reps
    tic; Az = zeros(5000); Bo = ones(5000); Ce = eye(5000); t = toc;  %#ok<NASGU>
    best_ac = min(best_ac, t);
end
fprintf('array_creation    : %.4f s   (no rand, best of %d)\n', best_ac, reps);

% loop -- non-foldable body at n = 20e6. See bench_fair.qu's loop_performance
% comment. MATLAB is the reason this kernel changed: its JIT folds the
% closed-form sum, so `result = result + i` at n=20e6 measures 0.0117 s
% (~2 cycles/iteration) against 0.5462 s for `result = result + mod(i,7)` --
% 47x apart at the same trip count. `i - 7*floor(i/7)` is the identical body
% used in all three ports; all three must print result = 60000003.
n_lp = 20000000;
warm = 0; for i = 1:1000, warm = warm + i - 7*floor(i/7); end
best_lp = inf;
for r = 1:reps
    tic; result = 0; for i = 1:n_lp, result = result + i - 7*floor(i/7); end; t = toc;
    best_lp = min(best_lp, t);
end
fprintf('loop_performance  : %.4f s   (n=%d, non-foldable, best of %d, result = %d)\n', best_lp, n_lp, reps, result);

fprintf('total             : %.4f s\n', best_mm + best_ew + best_fft + best_ac + best_lp);
