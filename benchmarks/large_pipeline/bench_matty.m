% bench_matty.m -- large-scale full signal-processing pipeline, Matty side.
%
% NOT the same file as bench.m -- Matty (checked matty_runner.py against
% bench.m first, per this task's own methodology) has no `rng()` (fails
% immediately, "Undefined function or variable 'rng'"), so results here
% are NOT seed-reproducible against the Qu/Python/MATLAB runs (own random
% stream each run) -- values will be close in distribution, not identical.
%
% Otherwise reuses bench.m's structure line for line, EXCEPT the filter
% stage: Matty's builtins.py has a real `butter` (scipy.signal-backed,
% returns [b,a] same convention as MATLAB's own) and `filter`, so this
% version uses the SAME order-4 Butterworth bandpass Qu/Python use,
% unlike bench.m's hand-rolled RBJ biquad substitute (this machine's real
% MATLAB license lacks Signal Processing Toolbox, so `butter` isn't
% available there -- Matty, ironically, has broader signal-processing
% coverage baked into its own interpreter than this particular MATLAB
% license does). `findpeaks`/`spectrogram`/`hamming` also exist in Matty,
% but this script keeps bench.m's manual, portable peak-finding and STFT
% loop rather than switching APIs mid-comparison.
%
% Run: python matty_runner.py bench_matty.m   (from the matty repo, with
% its own .venv active)

Fs = 10000;
N = 5000000;
csv_path = 'pipeline_signal_matty.csv';
features_path = 'pipeline_features_matty.csv';

% --- acquire + write CSV (untimed prep) --------------------------------------
% (Matty has no `table`/`readtable`/`writetable` -- checked; only
% `csvread`/`csvwrite` on plain matrices, same as file_io/'s CSV scenario.)
t = (0:N-1)' / Fs;
x_raw = sin(2*pi*60*t) + 0.6*sin(2*pi*440*t) + 0.3*sin(2*pi*1800*t) ...
    + 0.15*randn(N,1) + 0.5;
csvwrite(csv_path, [t x_raw]);

% --- stage 1: load -------------------------------------------------------------
tic;
D = csvread(csv_path);
xs = D(:, 2);
t_load = toc;

% --- stage 2: clean (remove DC offset) ------------------------------------------
tic;
xs_clean = xs - mean(xs);
t_clean = toc;

% --- stage 3: filter (bandpass, isolate 200-1000 Hz -- real order-4 Butterworth,
% same as Qu/Python -- Matty's butter/filter are scipy-backed) -----------------
tic;
[b, a] = butter(4, [200 1000] / (Fs/2), 'bandpass');
xs_filt = filter(b, a, xs_clean);
xs_filt = xs_filt(:);
t_filter = toc;

% --- stage 4: spectral analysis (rfft magnitude + peak-finding) -----------------
tic;
X = fft(xs_filt);
nyq = floor(N/2) + 1;
spectrum = abs(X(1:nyq));
min_height = max(spectrum) * 0.1;
left = [spectrum(1); spectrum(1:end-1)];
right = [spectrum(2:end); spectrum(end)];
is_peak = (spectrum > left) & (spectrum > right) & (spectrum > min_height);
cand = find(is_peak);
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
    p(p == 0) = 1.0;
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
csvwrite(features_path, [sig_rms sig_crest sig_energy length(peaks) mean(entropy)]);
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
