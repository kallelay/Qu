% bench.m -- large-scale full signal-processing pipeline, MATLAB side.
%
% IMPORTANT, honest caveat: this machine's MATLAB R2025b license does NOT
% include the Signal Processing Toolbox (checked via `ver` -- only base
% MATLAB + Parallel Computing Toolbox are licensed). `butter`, `findpeaks`,
% `spectrogram`, `hann`/`hamming` all hard-error ("... requires Signal
% Processing Toolbox") on this install. Rather than skip the MATLAB
% column, every gated piece below is hand-implemented from base MATLAB
% primitives (`fft`/`filter`/plain array ops), the same spirit as Qu's own
% native Rust implementations and scipy's compiled internals -- just
% written out here instead of being one call into a toolbox:
%
%   - bandpass filter: `butter`+`sosfilt`/`filter` (order-4, 8-pole) ->
%     a single RBJ "constant peak gain" biquad bandpass (2-pole), applied
%     via `filter()` (base MATLAB). NOT the same filter order/shape as
%     the Qu/Python side -- isolates the same 200-1000 Hz band, but with
%     a gentler rolloff, so downstream rms/crest/energy/entropy values
%     will be close but not matching to the same precision as the
%     Qu-vs-Python cross-check in ../full_pipeline/README.md. Flagged
%     here, not hidden.
%   - `findpeaks`: a vectorized local-maxima-above-threshold scan
%     (min_height only; min_distance enforced by a cheap loop afterward,
%     fine given ~1 peak is expected here, same as the Qu/Python runs).
%   - `spectrogram`/Hann window: a manual per-frame FFT loop with a
%     hand-coded Hann window (`0.5 - 0.5*cos(2*pi*n/(nfft-1))`) -- notably
%     NOT vectorized the way a toolbox call would be, so this stage's
%     MATLAB number reflects "interpreted loop over ~9766 frames", a
%     real, different cost shape than Qu's/Python's vectorized STFT, not
%     an apples-to-apples toolbox-vs-toolbox comparison.
%
% Run (cd into this script's own directory first, same reasoning as
% file_io/binary_doubles.m -- MATLAB's `run()` cd's into the script's own
% folder, which breaks repo-root-relative paths):
%   matlab -batch "cd('benchmarks/large_pipeline'); bench"

Fs = 10000;
N = 5000000;
csv_path = 'pipeline_signal_matlab.csv';
features_path = 'pipeline_features_matlab.csv';

% --- acquire + write CSV (untimed prep) --------------------------------------
rng(11);
t = (0:N-1)' / Fs;
x_raw = sin(2*pi*60*t) + 0.6*sin(2*pi*440*t) + 0.3*sin(2*pi*1800*t) ...
    + 0.15*randn(N,1) + 0.5;
T = table(t, x_raw, 'VariableNames', {'t', 'x'});
writetable(T, csv_path);

% --- stage 1: load -------------------------------------------------------------
tic;
df = readtable(csv_path);
xs = df.x;
t_load = toc;

% --- stage 2: clean (remove DC offset) ------------------------------------------
tic;
xs_clean = xs - mean(xs);
t_clean = toc;

% --- stage 3: filter (bandpass, isolate 200-1000 Hz -- hand-rolled RBJ biquad,
% Signal Processing Toolbox's butter/sosfilt unavailable on this install) -------
f0 = sqrt(200*1000);
BW = 1000 - 200;
Q = f0 / BW;
w0 = 2*pi*f0/Fs;
alpha = sin(w0) / (2*Q);
b0 = Q*alpha; b1 = 0; b2 = -Q*alpha;
a0 = 1 + alpha; a1 = -2*cos(w0); a2 = 1 - alpha;
bcoef = [b0 b1 b2] / a0;
acoef = [1 a1/a0 a2/a0];

tic;
xs_filt = filter(bcoef, acoef, xs_clean);
t_filter = toc;

% --- stage 4: spectral analysis (rfft magnitude + peak-finding) -----------------
tic;
X = fft(xs_filt);
nyq = floor(N/2) + 1;
spectrum = abs(X(1:nyq));
min_height = max(spectrum) * 0.1;
is_peak = false(nyq, 1);
is_peak(2:end-1) = spectrum(2:end-1) > spectrum(1:end-2) & spectrum(2:end-1) > spectrum(3:end);
is_peak = is_peak & (spectrum > min_height);
cand = find(is_peak);
% enforce min_distance=10 with a cheap greedy loop (few candidates expected)
peaks = [];
last = -Inf;
for i = 1:length(cand)
    if cand(i) - last >= 10
        peaks(end+1) = cand(i); %#ok<AGROW>
        last = cand(i);
    end
end
t_spectral = toc;

% --- stage 5: time-frequency (manual STFT loop + spectral entropy) -------------
tic;
nfft = 1024;
hop = 512;
n_frames = floor((N - nfft) / hop) + 1;
win = 0.5 - 0.5*cos(2*pi*(0:nfft-1)' / (nfft-1));  % hand-coded Hann window
nbins = floor(nfft/2) + 1;
entropy = zeros(n_frames, 1);
for i = 1:n_frames
    seg = xs_filt((i-1)*hop + 1 : (i-1)*hop + nfft) .* win;
    F = fft(seg);
    power = abs(F(1:nbins)) .^ 2;
    psum = sum(power);
    if psum == 0
        psum = 1.0;
    end
    p = power / psum;
    p(p == 0) = 1.0;  % avoid log2(0); those bins contribute 0
    entropy(i) = -sum(p .* log2(p)) / log2(nbins);
end
t_timefreq = toc;

% --- stage 6: feature extraction (STE, energy, crest factor, RMS) --------------
tic;
win_ste = 1024; hop_ste = 512;
n_ste = floor((N - win_ste) / hop_ste) + 1;
energy_frames = zeros(n_ste, 1);
for i = 1:n_ste
    seg = xs_filt((i-1)*hop_ste + 1 : (i-1)*hop_ste + win_ste);
    energy_frames(i) = sum(seg .^ 2);
end
sig_energy = sum(xs_filt .^ 2);
sig_rms = sqrt(mean(xs_filt .^ 2));
sig_crest = max(abs(xs_filt)) / sig_rms;
t_features = toc;

% --- stage 7: export extracted features -----------------------------------------
tic;
features = table(sig_rms, sig_crest, sig_energy, length(peaks), mean(entropy), ...
    'VariableNames', {'rms', 'crest_factor', 'energy', 'n_peaks', 'mean_entropy'});
writetable(features, features_path);
t_export = toc;

t_total = t_load + t_clean + t_filter + t_spectral + t_timefreq + t_features + t_export;

fprintf('load       : %.4f s\n', t_load);
fprintf('clean      : %.4f s\n', t_clean);
fprintf('filter     : %.4f s\n', t_filter);
fprintf('spectral   : %.4f s\n', t_spectral);
fprintf('timefreq   : %.4f s\n', t_timefreq);
fprintf('features   : %.4f s\n', t_features);
fprintf('export     : %.4f s\n', t_export);
fprintf('TOTAL      : %.4f s\n', t_total);
fprintf('\n');
fprintf('rms=%.4f crest=%.4f energy=%.2f n_peaks=%d mean_entropy=%.4f\n', ...
    sig_rms, sig_crest, sig_energy, length(peaks), mean(entropy));
