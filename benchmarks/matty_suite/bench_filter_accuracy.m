% bench_filter_accuracy.m -- MATLAB port of bench_filter_accuracy.qu.
%
% Same closed-form ground truth, same explicit frequency axis (0:pi,
% n_pts points, passed directly to freqz) so every language evaluates
% the identical set of frequencies -- freqz's own default point count
% does not guarantee the same axis another language's default would.
%
% UNTESTED on this machine: R2025b (the only licensed, working install --
% R2026a's own license is separately expired) has no Signal Processing
% Toolbox (checked via `ver`; only MATLAB + Parallel Computing Toolbox are
% licensed), and both `butter` and `freqz` require it. Written to match
% bench_filter_accuracy.qu/.py exactly so it is ready the moment a
% licensed install is available -- do not report a result from this file
% without actually running it first.

Fs = 1000.0;
fc = 100.0;
n_pts = 513;
wc = 2.0 * pi * fc / Fs;
orders = [2, 4, 6, 8, 10, 12, 16, 20];
w = linspace(0, pi, n_pts)';

fprintf('order  max_err       rms_err       design+freqz time\n');
for N = orders
    tic;
    sos = butter(N, fc / (Fs / 2), 'low');
    H = freqz(sos, w);
    t = toc;

    mag = abs(H);
    exact_mag = ones(n_pts, 1);
    nz = w > 0;
    ratio = tan(w(nz) / 2.0) / tan(wc / 2.0);
    exact_mag(nz) = sqrt(1.0 ./ (1.0 + ratio .^ (2 * N)));

    err = abs(mag - exact_mag);
    max_err = max(err);
    rms_err = sqrt(mean(err .^ 2));

    fprintf('%2d     %.6e  %.6e  %.6f s\n', N, max_err, rms_err, t);
end
