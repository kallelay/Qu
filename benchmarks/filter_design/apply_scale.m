% apply_scale.m -- filter APPLICATION at large scale, MATLAB/Matty port of
% apply_scale.qu. Same Butterworth lowpass (order 4, 500 Hz, Fs = 10 kHz),
% same 1M/10M sample scales.
%
% Written to run UNMODIFIED under both real MATLAB and Matty, but the two
% take genuinely different code paths through this script, guarded by
% try/catch + exist() checks -- documented here rather than left implicit:
%
%   - Matty has butter/filter/filtfilt (see design.m's own header) and
%     takes the "real" path: designs the filter itself via butter(), then
%     times filter() (causal) and filtfilt() (zero-phase).
%   - This machine's real MATLAB R2025b install has NO Signal Processing
%     Toolbox (confirmed directly: `ver` lists only base MATLAB + Parallel
%     Computing Toolbox; `license('test','signal_toolbox')` reports the
%     license entitlement as available, but the toolbox itself is not
%     installed) -- butter() and filtfilt() both throw
%     "Unrecognized function". Falls back to (a) literal, precomputed [b,a]
%     coefficients for the identical filter (same order-4/500Hz/10kHz
%     Butterworth lowpass, computed via scipy.signal.butter and
%     cross-checked against Qu's own `butter` design elsewhere in this
%     benchmark suite -- NOT computed by this MATLAB install, since it
%     can't), then (b) `filter()` (base MATLAB, always available) for the
%     causal case, and a hand-rolled forward+backward double filter for
%     the zero-phase case -- NOT a faithful `filtfilt` (real `filtfilt`
%     also does edge-padding/reflection and initial-condition matching
%     that this manual version skips), labeled "manual_filtfilt" rather
%     than "filtfilt" throughout so this distinction isn't lost in the
%     results table.

Fs = 10000;
N_list = [1000000, 10000000];
seeds = [21, 22];

have_toolbox = (exist('butter') > 0) && (exist('filtfilt') > 0);

if have_toolbox
    disp('Signal Processing Toolbox available -- designing via butter()/filtfilt()')
    Wn = 500 / (Fs/2);
    [b, a] = butter(4, Wn, 'low');
else
    disp('Signal Processing Toolbox NOT available on this MATLAB install -- using precomputed [b,a] + manual filtfilt')
    b = [0.00041659920440659937, 0.0016663968176263975, 0.002499595226439596, 0.0016663968176263975, 0.00041659920440659937];
    a = [1.0, -3.180638548874719, 3.8611943489942133, -2.112155355110969, 0.43826514226197977];
end

for k = 1:2
    N = N_list(k);
    seed_val = seeds(k);
    % Matty has no `rng()` builtin (checked src/builtins.py -- not
    % registered) -- guarded rather than hard-failing there; the RMS
    % sanity check below doesn't depend on bit-exact noise, only on the
    % three tones dominating, so an unseeded draw under Matty is fine.
    if exist('rng') > 0
        rng(seed_val);
    end

    t = linspace(0, (N-1)/Fs, N);
    x = sin(2*pi*50*t) + 0.5*sin(2*pi*500*t) + 0.25*sin(2*pi*2000*t) + 0.1*randn(1, N);

    tic;
    y_causal = filter(b, a, x);
    t_causal = toc;
    fprintf('filter          N=%d  : %.4f s\n', N, t_causal);

    if have_toolbox
        tic;
        y_zp = filtfilt(b, a, x);
        t_zp = toc;
        fprintf('filtfilt        N=%d  : %.4f s\n', N, t_zp);
    else
        tic;
        y_fwd = filter(b, a, x);
        y_zp = fliplr(filter(b, a, fliplr(y_fwd)));
        t_zp = toc;
        fprintf('manual_filtfilt N=%d  : %.4f s\n', N, t_zp);
    end

    fprintf('rms(y_causal) = %.4f\n', sqrt(mean(y_causal.^2)));
end
