% bridge_shm.m -- MATLAB equivalent of catalog/qu_bridge_shm.qu and
% bridge_shm.py: same synthetic bridge-SHM simulation and the same
% spectral-analysis damage-detection + TDOA sanity-check pipeline.
%
% Signal Processing Toolbox is NOT actually installed on this machine
% (checked directly: `which pwelch/hilbert/findpeaks` all report "not
% found" even though `license('test','Signal_Toolbox')` returns true --
% license entitlement isn't the same as the toolbox being installed).
% `xcorr` IS available (it ships in base MATLAB's `datafun`, not gated by
% the toolbox), so it is used directly for the TDOA step, matching
% Qu/SciPy's `xcorr`/`correlate` usage exactly (verified below to share
% the same lag-sign convention). `periodogram`/`hilbert`/`findpeaks` are
% re-implemented here from scratch using only base MATLAB (`fft`/`ifft`),
% following the exact same formulas Qu's own implementation uses (see
% `qu-core/src/transforms.rs`: periodic-Hann-windowed |FFT|^2/scale for
% the periodogram, one-sided-spectrum construction for the analytic
% signal, greedy-by-height suppression for peak picking).
%
% NOTE ON NOISE: reproducing Qu's exact PRNG (splitmix64 -> Box-Muller) in
% MATLAB was skipped for this comparison -- MATLAB's own `rng(seed)` +
% `randn` is used instead, so the per-sensor noise REALIZATION differs
% sample-for-sample from the Qu/Python runs (which share an exact,
% verified-bit-identical PRNG). The modal content, mode shapes, damage
% event, and vehicle-bump timing are identical parameters; only the small
% (sigma=0.05) measurement-noise draws differ. See
% `catalog/qu_bridge_shm.qu`'s header for the full simulation writeup.

%% Bridge & sensor geometry
L = 40.0;
xs = [4.0, 10.0, 16.0, 24.0, 30.0, 36.0];
n_sensors = length(xs);

fs = 200.0;
duration = 120.0;
N = round(duration * fs);
N_half = N / 2;
t = (0:N-1) / fs;
t_local = (0:N_half-1) / fs;

t_anomaly = 60.0;

%% Modal content (standing waves, no propagation delay)
mode_n = [1, 2, 3];
f_healthy = [2.10, 5.80, 11.30];
f_damaged = [1.98, 5.80, 11.30];
A_mode = [1.0, 0.6, 0.35];

%% Moving-load traveling bump (separate physics, for TDOA)
v_vehicle = 20.0;
t_events = [20.0, 90.0];
bump_width = 0.10;
bump_amp = 8.0;

noise_sigma = 0.05;
noise_seed_base = 9001;

%% Synthesize each sensor's recording
tic;
mem_samples = get_mem_used_mb();

Y = zeros(n_sensors, N);
for si = 1:n_sensors
    x_s = xs(si);

    y_before = zeros(1, N_half);
    y_after = zeros(1, N_half);
    for mi = 1:length(mode_n)
        n = mode_n(mi);
        phi = sin(n * pi * x_s / L);
        amp = A_mode(mi);
        y_before = y_before + amp * phi * sin(2*pi*f_healthy(mi)*t_local);
        y_after  = y_after  + amp * phi * sin(2*pi*f_damaged(mi)*t_local);
    end
    y_modal = [y_before, y_after];

    bump = zeros(1, N);
    for ei = 1:length(t_events)
        t_arrive = t_events(ei) + x_s / v_vehicle;
        bump = bump + bump_amp * exp(-((t - t_arrive).^2) / (2*bump_width^2));
    end

    rng(noise_seed_base + si - 1, 'twister');
    noise = noise_sigma * randn(1, N);

    Y(si, :) = y_modal + bump + noise;
end

mem_samples(end+1) = get_mem_used_mb();
fprintf('[MATLAB] synthesized %d sensors x %d samples (%.0f s at %.0f Hz)\n', n_sensors, N, duration, fs);

%% Step 1-2: per-sensor spectral analysis + frequency-shift detection
band_lo = 1.5;
band_hi = 2.5;
df = fs / N_half;
idx_lo = round(band_lo / df);   % 0-indexed bin
idx_hi = round(band_hi / df);

shift_threshold = 0.05;
shifts = zeros(1, n_sensors);

fprintf('\n=== Step 1-2: modal frequency-shift damage detection ===\n');
for si = 1:n_sensors
    before = Y(si, 1:N_half);
    after  = Y(si, N_half+1:N);

    psd_before = my_periodogram(before, fs);
    psd_after  = my_periodogram(after, fs);

    % idx_lo/idx_hi are 0-indexed bins -> MATLAB slice is (idx_lo+1):(idx_hi+1)
    band_before = psd_before(idx_lo+1:idx_hi+1);
    band_after  = psd_after(idx_lo+1:idx_hi+1);

    [~, rel_before] = max(band_before);
    [~, rel_after]  = max(band_after);
    peak_before = idx_lo + (rel_before - 1);
    peak_after  = idx_lo + (rel_after - 1);

    f_before = peak_before * df;
    f_after  = peak_after * df;
    shift = f_after - f_before;
    shifts(si) = shift;

    if abs(shift) > shift_threshold
        flag = 'damage suspected';
    else
        flag = 'no significant shift';
    end
    fprintf('sensor %d (x=%.0f m): mode-1 peak before=%.3f Hz, after=%.3f Hz, shift=%.3f Hz -> %s\n', ...
        si-1, xs(si), f_before, f_after, shift, flag);
end

mean_shift = mean(shifts);
fprintf('\nmean detected mode-1 shift across sensors: %.3f Hz (injected: %.3f Hz)\n', ...
    mean_shift, f_damaged(1) - f_healthy(1));

expected_shift = f_damaged(1) - f_healthy(1);
if abs(mean_shift - expected_shift) < 0.03 && mean_shift < -shift_threshold
    fprintf('CHECK PASSED: detected frequency shift matches the injected damage within tolerance.\n');
else
    fprintf('CHECK FAILED: detected shift does not match the injected damage.\n');
end
mem_samples(end+1) = get_mem_used_mb();

%% Step 3: TDOA sanity check via cross-correlation
fprintf('\n=== Step 3: TDOA sanity check (vehicle-bump cross-correlation) ===\n');

ref = 1;  % MATLAB 1-indexed: sensor xs(1), first to feel a crossing

Env = zeros(n_sensors, N);
for si = 1:n_sensors
    Env(si, :) = abs(my_hilbert(Y(si, :)));
end

env_ref = Env(ref, :);

detect_threshold = mean(env_ref) + 4*std(env_ref);
locs = my_findpeaks(env_ref, detect_threshold, round(5*fs));
fprintf('detected %d vehicle-crossing event(s) on the reference sensor\n', length(locs));

win_pre = 1.0;
win_post = 3.0;
delay_errors = [];

win_len_full = round((win_pre + win_post) * fs);
tau_template = (0:win_len_full-1) / fs;
template = exp(-((tau_template - win_pre).^2) / (2*bump_width^2));
template = template - mean(template);

for ev = 1:length(locs)
    t_ref_arrive = (locs(ev) - 1) / fs;   % locs is 1-indexed
    lo = round((t_ref_arrive - win_pre) * fs);   % 0-indexed sample
    hi = round((t_ref_arrive + win_post) * fs);
    lo = max(lo, 0);
    hi = min(hi, N-1);

    w_ref = Env(ref, lo+1:hi+1);
    w_ref = w_ref - mean(w_ref);

    [c_ref, lags_ref] = xcorr(template, w_ref);
    [~, i_ref] = max(c_ref);
    peak_ref_lag = lags_ref(i_ref);

    fprintf('\nevent at t~%.2f s (window [%.2f, %.2f] s):\n', t_ref_arrive, lo/fs, hi/fs);
    for si = 2:n_sensors
        w_i = Env(si, lo+1:hi+1);
        w_i = w_i - mean(w_i);

        [c_i, lags_i] = xcorr(template, w_i);
        [~, i_i] = max(c_i);
        peak_i_lag = lags_i(i_i);

        lag_samples = peak_ref_lag - peak_i_lag;
        tau_est = lag_samples / fs;

        tau_true = (xs(si) - xs(ref)) / v_vehicle;
        err = tau_est - tau_true;
        delay_errors(end+1) = err; %#ok<AGROW>
        fprintf('  sensor %d (x=%.0f m): tau_est=%.4f s, tau_true=%.4f s, err=%.2f ms\n', ...
            si-1, xs(si), tau_est, tau_true, err*1000);
    end
end

mem_samples(end+1) = get_mem_used_mb();
tdoa_tolerance_samples = 8;
max_abs_err_samples = max(abs(delay_errors)) * fs;
fprintf('\nmax |TDOA error| across sensor pairs, all events: %.2f samples (%.2f ms)\n', ...
    max_abs_err_samples, max(abs(delay_errors))*1000);
if max_abs_err_samples < tdoa_tolerance_samples
    fprintf('CHECK PASSED: TDOA estimates match the known geometric delay within %d samples.\n', tdoa_tolerance_samples);
else
    fprintf('CHECK FAILED: TDOA estimates deviate from the known geometric delay by more than %d samples.\n', tdoa_tolerance_samples);
end

mem_samples(end+1) = get_mem_used_mb();
elapsed_s = toc;
fprintf('\n[MATLAB] elapsed=%.2f ms for simulate+analyze %d sensors x %d samples\n', elapsed_s*1000, n_sensors, N);

%% Memory report: `memory` (Windows-only, confirmed available here --
%% `feature('memstats')` also runs without erroring but only dumps
%% whole-SYSTEM physical/virtual memory stats, not a per-process number,
%% so it isn't usable for a peak-RSS-style comparison) reports
%% `MemUsedMATLAB`, this process's actual memory use, checked at a few
%% points through the run above; the max of those stands in for
%% Python's continuous psutil-sampled peak (MATLAB has no lightweight
%% background-thread sampler in base `-batch` mode).
fprintf('[MATLAB] peak RSS (max of %d MemUsedMATLAB samples) = %.1f MB\n', length(mem_samples), max(mem_samples));

%% Plot (not included in the timed comparison)
fig = figure('Visible', 'off');

subplot(2,2,1);
plot(t(1:2000), Y(1,1:2000), 'b', 'DisplayName', sprintf('sensor 0 (x=%.0f m)', xs(1))); hold on;
plot(t(1:2000), Y(4,1:2000), 'r', 'DisplayName', sprintf('sensor 3 (x=%.0f m)', xs(4)));
xlabel('Time (s)'); ylabel('Amplitude'); title('Time-domain traces (first 10 s)'); legend;

subplot(2,2,2);
t_ev = t_events(1);
lo2 = round((t_ev - 0.5) * fs);
hi2 = round((t_ev + 3.0) * fs);
plot(t(lo2+1:hi2+1), Y(1,lo2+1:hi2+1), 'b', 'DisplayName', 'sensor 0'); hold on;
plot(t(lo2+1:hi2+1), Y(6,lo2+1:hi2+1), 'g', 'DisplayName', sprintf('sensor 5 (x=%.0f m)', xs(6)));
xlabel('Time (s)'); ylabel('Amplitude'); title('Vehicle-crossing bump: arrival delay'); legend;

subplot(2,2,3);
sensor_plot = 3;  % 0-indexed sensor 2 -> MATLAB index 3
before_p = Y(sensor_plot, 1:N_half);
after_p  = Y(sensor_plot, N_half+1:N);
psd_b = my_periodogram(before_p, fs);
psd_a = my_periodogram(after_p, fs);
freq_axis = linspace(0, fs/2, length(psd_b));
plot(freq_axis(1:300), psd_b(1:300), 'b', 'DisplayName', 'before (healthy)'); hold on;
plot(freq_axis(1:300), psd_a(1:300), 'r', 'DisplayName', 'after (damaged)');
xlabel('Frequency (Hz)'); ylabel('PSD'); title(sprintf('sensor %d: before/after PSD (mode-1 shift)', sensor_plot-1)); legend;

subplot(2,2,4);
% spectrogram() needs Signal Processing Toolbox (not installed here --
% see header note); a minimal base-MATLAB STFT stands in for it.
[S, spec_freqs, spec_times] = my_spectrogram(Y(sensor_plot,:), 4096, 1024, fs);
imagesc(spec_times, spec_freqs, 20*log10(abs(S) + 1e-12));
axis xy;
ylim([0 15]);
colormap(gca, 'parula');
title(sprintf('sensor %d: spectrogram (mode-1 shift at t=%.0f s)', sensor_plot-1, t_anomaly));

saveas(fig, 'bridge_shm_matlab.svg');
fprintf('[MATLAB] wrote bridge_shm_matlab.svg\n');

%% ---- Local functions (base-MATLAB re-implementations) --------------------
function mb = get_mem_used_mb()
    if ispc
        mem_info = memory;
        mb = mem_info.MemUsedMATLAB / (1024*1024);
    else
        mb = NaN;
    end
end

function psd = my_periodogram(x, fs)
    % Periodic-Hann-windowed |FFT|^2 / (fs * sum(w.^2)), doubled on every
    % bin except DC and Nyquist (even length) -- matches Qu's
    % qu-core/src/transforms.rs `welch`/`periodogram` and SciPy's own
    % `scaling="density"` convention exactly.
    x = x(:);
    N = length(x);
    i = (0:N-1)';
    w = 0.5 - 0.5*cos(2*pi*i/N);   % periodic Hann
    s2 = sum(w.^2);
    scale = fs * s2;
    X = fft(x .* w);
    nbins = floor(N/2) + 1;
    X = X(1:nbins);
    p = (abs(X).^2) / scale;
    is_even = mod(N, 2) == 0;
    for k = 2:nbins
        if ~(is_even && k == nbins)
            p(k) = p(k) * 2;
        end
    end
    psd = p;
end

function xa = my_hilbert(x)
    % Analytic signal via the standard one-sided-spectrum construction:
    % DC (and Nyquist, if N even) unchanged, positive frequencies doubled,
    % negative frequencies zeroed -- matches Qu's qu-core `hilbert` and
    % SciPy's `scipy.signal.hilbert`.
    x = x(:);
    N = length(x);
    X = fft(x);
    half = floor(N/2);
    is_even = mod(N, 2) == 0;
    H = zeros(N, 1);
    H(1) = 1;   % k=0 (DC)
    for k = 1:N-1
        if is_even
            if k < half
                f = 2;
            elseif k == half
                f = 1;
            else
                f = 0;
            end
        else
            if k <= half
                f = 2;
            else
                f = 0;
            end
        end
        H(k+1) = f;
    end
    xa = ifft(X .* H);
    xa = xa(:)';
end

function [S, freqs, times] = my_spectrogram(x, nfft, hop, fs)
    % Minimal periodic-Hann STFT magnitude, positive-frequency half only
    % -- a base-MATLAB stand-in for the Signal Processing Toolbox's
    % `spectrogram`, used only for the plot panel (not part of any
    % measured/checked result).
    x = x(:)';
    N = length(x);
    i = (0:nfft-1);
    w = 0.5 - 0.5*cos(2*pi*i/nfft);
    n_frames = floor((N - nfft) / hop) + 1;
    nbins = floor(nfft/2) + 1;
    S = zeros(nbins, n_frames);
    for fr = 1:n_frames
        start_idx = (fr-1)*hop + 1;
        seg = x(start_idx:start_idx+nfft-1) .* w;
        X = fft(seg);
        S(:, fr) = X(1:nbins);
    end
    freqs = (0:nbins-1) * fs / nfft;
    times = ((0:n_frames-1)*hop + nfft/2) / fs;
end

function locs = my_findpeaks(x, min_height, min_distance)
    % Local maxima at or above min_height, greedy-by-height suppression
    % within min_distance samples -- matches Qu's find_peaks/findpeaks
    % (qu-core/src/transforms.rs) "keep the tallest of a close cluster"
    % rule. Returns 1-indexed locations, ascending.
    x = x(:)';
    n = length(x);
    cand = [];
    for i = 2:n-1
        if x(i) > x(i-1) && x(i) > x(i+1) && x(i) >= min_height
            cand(end+1) = i; %#ok<AGROW>
        end
    end
    if isempty(cand)
        locs = [];
        return;
    end
    [~, order] = sort(x(cand), 'descend');
    cand_sorted = cand(order);
    selected = [];
    for ci = 1:length(cand_sorted)
        c = cand_sorted(ci);
        ok = true;
        for si = 1:length(selected)
            if abs(c - selected(si)) < min_distance
                ok = false;
                break;
            end
        end
        if ok
            selected(end+1) = c; %#ok<AGROW>
        end
    end
    locs = sort(selected);
end
