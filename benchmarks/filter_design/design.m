% design.m -- filter DESIGN benchmark, MATLAB/Matty port of design.qu.
%
% Same scenarios (butter/cheby1/cheby2/ellip at order 4/8/16, fir1/firls at
% order 64/256/1024), same repeated-loop-average timing methodology.
%
% Written to run UNMODIFIED under both:
%   - real MATLAB (R2025b, Signal Processing Toolbox):
%       matlab -batch "run('benchmarks/filter_design/design.m')"
%   - Matty (a sibling `matty` checkout, a separate
%     MATLAB/Octave-compatible interpreter):
%       python matty_runner.py <path-to-this-file>
%
% Matty's butter/cheby1/cheby2/ellip/fir1 do NOT support the `fs=`/`'sos'`
% Hz-and-SOS convention Qu and scipy use above -- only the classic
% normalized-frequency, transfer-function ([b,a]) form real MATLAB has
% always had too. So this script deliberately uses that lowest-common-
% denominator form (Wn = cutoff/(Fs/2), plain [b,a] outputs) instead of
% `[sos,g] = butter(...)` -- it is still exactly what a MATLAB user without
% `output='sos'` habits would write, not a crippled version of the design
% step. Matty has no `firls` builtin at all -- that scenario is skipped
% under Matty (see README for what to do there).

Fs = 10000;
cutoff = 500;         % Hz, lowpass -- 0.1 * Nyquist (Nyquist = Fs/2 = 5000)
Wn = cutoff / (Fs/2); % normalized cutoff, 1 = Nyquist -- MATLAB's own Wn convention
rp = 1;                % dB passband ripple (cheby1, ellip)
rs = 40;               % dB stopband attenuation (cheby2, ellip)

disp('=== IIR filter design: butter / cheby1 / cheby2 / ellip, order 4/8/16 ===')

orders = [4, 8, 16];
reps_iir = [5000, 2000, 500];

for oi = 1:3
    n = orders(oi);
    reps = reps_iir(oi);

    tic;
    for i = 1:reps
        b = butter(n, Wn, 'low');
    end
    t = toc / reps;
    fprintf('butter  order=%-3d: %.2f us/call  (%d reps)\n', n, t*1e6, reps);

    tic;
    for i = 1:reps
        b = cheby1(n, rp, Wn, 'low');
    end
    t = toc / reps;
    fprintf('cheby1  order=%-3d: %.2f us/call  (%d reps)\n', n, t*1e6, reps);

    tic;
    for i = 1:reps
        b = cheby2(n, rs, Wn, 'low');
    end
    t = toc / reps;
    fprintf('cheby2  order=%-3d: %.2f us/call  (%d reps)\n', n, t*1e6, reps);

    tic;
    for i = 1:reps
        b = ellip(n, rp, rs, Wn, 'low');
    end
    t = toc / reps;
    fprintf('ellip   order=%-3d: %.2f us/call  (%d reps)\n', n, t*1e6, reps);
end

disp(' ')
disp('=== FIR filter design: fir1 / firls, order 64/256/1024 (taps = order+1) ===')

fir_orders = [64, 256, 1024];
reps_fir1 = [5000, 3000, 1000];

for oi = 1:3
    n = fir_orders(oi);
    reps = reps_fir1(oi);
    tic;
    for i = 1:reps
        b = fir1(n, 0.1);
    end
    t = toc / reps;
    fprintf('fir1    order=%-4d: %.2f us/call  (%d reps)\n', n, t*1e6, reps);
end

% firls: real MATLAB has it (Signal Processing Toolbox); Matty does not
% (checked TODO.md/src/builtins.py -- no firls entry at all) -- guarded so
% this same script doesn't hard-error under Matty, it just skips + reports.
if exist('firls') > 0
    reps_firls = [30, 3, 1];
    for oi = 1:3
        n = fir_orders(oi);
        reps = reps_firls(oi);
        tic;
        for i = 1:reps
            b = firls(n, [0, 0.08, 0.12, 1], [1, 1, 0, 0]);
        end
        t = toc / reps;
        fprintf('firls   order=%-4d: %.2f us/call  (%d reps)\n', n, t*1e6, reps);
    end
else
    disp('firls: not available in this interpreter, skipped')
end
