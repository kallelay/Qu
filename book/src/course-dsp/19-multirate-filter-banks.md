# Lesson 19 — Multirate DSP and filter banks

## The idea

A sampled signal carries an implicit contract: so many numbers per second,
fixed at capture time. Two systems in a chain rarely agree on that number —
a sensor logs at whatever rate the physics demands, a codec ships at
whatever rate its channel affords — so converting a signal from one sample
rate to another, without distorting it, is one of the most common
operations in practical DSP.

Rate conversion by an integer factor decomposes into exactly two primitive
operations.

**Upsampling** by \(L\) raises the rate by inserting new samples between
the existing ones. Guessing their values is not an option; the well-defined
version inserts zeros (zero-stuffing) and then low-pass filters:

\[
v[n] = \begin{cases} x[n/L] & n \bmod L = 0 \\ 0 & \text{otherwise}
\end{cases}, \qquad y[n] = L \cdot (v * h_{\text{lp}})[n]
\]

`h_lp` cuts off at the original Nyquist frequency, expressed relative to
the new, higher rate. Zero-stuffing alone divides the signal's average
power by \(L\) (most samples are now exactly zero); the filter interpolates
smooth values between them, and multiplying by \(L\) afterward restores the
amplitude.

**Downsampling** by \(M\) lowers the rate by discarding samples — but only
after low-pass filtering to the new Nyquist frequency:

\[
y[n] = (x * h_{\text{lp}})[nM], \qquad h_{\text{lp}} \text{ cutoff } =
\frac{f_s}{2M}
\]

Skip that filter and any energy above the new Nyquist folds back and
reappears as a lower frequency that was never in the original signal —
aliasing, from Lesson 3, triggered this time by throwing samples away
rather than by under-sampling in the first place.

```qu
seed(2)
fs = 400
n = 400
t = (0 to n - 1) / fs
x = sin(2*pi*30*t) + sin(2*pi*140*t)
M = 4
fs_new = fs / M

naive = zeros(n / M)
for i in 0 to n/M - 1
    naive[i] = x[i * M]
end for

aa = fir1(60, fs_new/2, kind="low", fs=fs)
xf = sosfilt(aa, x)
clean = zeros(n / M)
for i in 0 to n/M - 1
    clean[i] = xf[i * M]
end for

mag_naive = abs(rfft(naive))
mag_clean = abs(rfft(clean))
f_new = (0 to length(mag_naive)-1) * fs_new / (n/M)
bin40 = round(40 / (fs_new / (n/M)))
print("new Nyquist: {fs_new/2} Hz")
print("energy near 40 Hz (aliased 140 Hz tone) -- no filter: {round(mag_naive[bin40],3)}, with filter: {round(mag_clean[bin40],3)}")

figure()
plot(f_new, mag_naive, color = "#e11d48", width = 2, label = "decimated, no anti-alias filter")
plot(f_new, mag_clean, color = "#0f172a", width = 2, label = "decimated, with anti-alias filter")
xlabel("frequency (Hz)")
ylabel("|X(f)|")
title("Decimation by 4: a 140 Hz tone above the new 50 Hz Nyquist")
legend()
```

```
new Nyquist: 50 Hz
energy near 40 Hz (aliased 140 Hz tone) -- no filter: 50, with filter: 0.561
```

A 30 Hz tone and a 140 Hz tone, decimated by 4 to a 100 Hz rate whose
Nyquist is 50 Hz. Skipping the anti-alias filter leaves the 140 Hz tone
folded down to \(|140 - 100| = 40\) Hz, indistinguishable from a real 40 Hz
signal — energy 50 at that bin. Filtering first removes it almost
completely — energy 0.561, nearly two orders of magnitude down.

Chaining decimators, narrow-band filters, and expanders together into a
**filter bank** lets each sub-band run at its own, lower rate instead of
the shared input rate — part of why an MP3 encoder can spend more bits
where the ear is more sensitive. Ronald Crochiere and Lawrence Rabiner
worked out efficient multirate structures at Bell Labs through the 1970s
and collected the field in their 1983 book, *Multirate Digital Signal
Processing*.

## In Qu

Qu's standard library has no `upsample`, `downsample`, or `polyphase`
builtin — the two primitives above are built directly from `fir1` and
`sosfilt`. Take a 1000 Hz signal carrying two tones and upsample it to
3000 Hz, then decimate it straight back down, comparing the round trip
against the original:

```qu
seed(1)
fs_lo = 1000
L = 3
fs_hi = fs_lo * L
n = 200
t = (0 to n - 1) / fs_lo
x = sin(2*pi*90*t) + 0.3*sin(2*pi*310*t)

up = zeros(n * L)
for i in 0 to n - 1
    up[i * L] = x[i]
end for
interp_filt = fir1(60, fs_lo/2, kind="low", fs=fs_hi)
y_up = L * sosfilt(interp_filt, up)

aa_filt = fir1(60, fs_lo/2, kind="low", fs=fs_hi)
filtered = sosfilt(aa_filt, y_up)
y_down = zeros(n)
for i in 0 to n - 1
    y_down[i] = filtered[i * L]
end for

delay = 20
err = rmse(x[0:n-delay-1], y_down[delay:n-1])
print("upsampled length: {length(y_up)} (expect {n*L})")
print("round-trip rmse (delay-aligned): {round(err, 5)}")

zoom_lo = 0
zoom_hi = 60
t_hi = (0 to n*L - 1) / fs_hi
figure()
plot(t[zoom_lo:zoom_hi], x[zoom_lo:zoom_hi], color = "#0f172a", width = 2, label = "original (1000 Hz)")
plot(t_hi[zoom_lo*L:zoom_hi*L], y_up[zoom_lo*L:zoom_hi*L], color = "#5B7CFA", width = 1, label = "upsampled (3000 Hz)")
plot(t[zoom_lo:zoom_hi-delay], y_down[zoom_lo+delay:zoom_hi], color = "#e11d48", width = 1.5, label = "round-tripped")
xlabel("time (s)")
ylabel("amplitude")
title("Upsample by 3, then downsample by 3")
legend()
```

```
upsampled length: 600 (expect 600)
round-trip rmse (delay-aligned): 0.00249
```

The round trip does not land on exactly zero error, and `delay = 20` is
why: each order-60 FIR filter delays a signal by half its order, 30 samples
at the 3000 Hz rate, which is 20 samples once rescaled back to 1000 Hz.
Compared against the original shifted by that known delay, the recovered
signal comes back within a quarter of one percent — windowing and
finite-order error, not a bug, shrinking further at higher filter order.
The two waveforms overlay almost exactly once that shift is accounted for;
the spectrum tells the same story more bluntly:

```qu
mag_orig = abs(rfft(x))
mag_rec = abs(rfft(y_down[delay:n-1]))
f_orig = (0 to length(mag_orig)-1) * fs_lo / n
f_rec = (0 to length(mag_rec)-1) * fs_lo / (n - delay)
figure()
plot(f_orig, mag_orig, color = "#0f172a", width = 2, label = "original spectrum")
plot(f_rec, mag_rec, color = "#e11d48", width = 1.5, label = "round-tripped spectrum")
xlabel("frequency (Hz)")
ylabel("|X(f)|")
title("Spectral content survives the round trip")
legend()
```

Both peaks, 90 Hz and 310 Hz, land in the same place before and after —
upsampling then downsampling by the same factor is lossy only at the
sub-percent level measured above, not in the frequencies it carries.

Qu does provide one higher-level function outside this hand-built pair:
`resample_to(signal, new_fs)`.

```qu
sig = signal(x, fs_lo)
r = resample_to(sig, 3000)
print("resample_to length: {length(r)} (zero-stuff+filter gave {n*L})")
```

```
resample_to length: 598 (zero-stuff+filter gave 600)
```

Not 600 — `resample_to` spans the same duration with new instants rather
than inserting `L` samples per old one, and it does so by evaluating a
linear interpolant, with no anti-aliasing step at all. Going up in rate
that is a soft but tolerable approximation; going *down*, it performs no
filtering before re-evaluating the signal more sparsely. Feed it a
wideband signal and ask for a lower rate, and whatever lives above the new
Nyquist limit aliases silently — exactly the failure this lesson built
machinery to avoid. `resample_to` is the right tool for a slight clock
correction; it is not a decimator.

Everything here assumed you get to choose how much to sample and discard
part of it afterward. The next lesson asks the sharper question: what if
acquiring at the full rate is the expensive, or impossible, part? If a
signal is sparse enough, most of those samples were never needed at all.
