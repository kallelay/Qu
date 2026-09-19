# Lesson 6 — Time-Frequency & Wavelets

## The idea

An ambulance siren does not have a frequency. It has a frequency *right
now*, a different one a second ago, and a different one a second from now
— the rising-then-falling wail your ear uses to tell whether it is
approaching or receding. Ask "what frequency is the siren" and there is no
single number to give back.

Lesson 5 closed on exactly this gap: the DFT, the FFT, poles sitting fixed
on the s-plane or z-plane all describe signals and systems that hold
still. A single Fourier transform of a whole recording collapses every
instant into one spectrum, discarding whatever timing distinguished an
early frequency from a later one in the same step that built the frequency
axis.

```qu
fs = 100
N = 200
t = (0 to N - 1) / fs
low_then_high = cat(1, cos(2 * pi * 4 * t[0:99]), cos(2 * pi * 18 * t[100:199]))
figure()
subplot(1, 2, 1)
plot(t, low_then_high)
xlabel("time (s)")
ylabel("amplitude")
mag = abs(rfft(low_then_high))
f = (0 to length(mag) - 1) * fs / N
subplot(1, 2, 2)
plot(f, mag)
xlabel("frequency (Hz)")
ylabel("magnitude")
```

A 4 Hz tone followed by an 18 Hz tone (left) produces a spectrum (right)
with two peaks and no indication that one happened before the other — the
same signal, played in the opposite order, would give an identical
picture.

The fix: instead of transforming the whole recording at once, slide a
short window along it and take a separate spectrum of each slice. Line the
spectra up side by side — time across, frequency up, brightness for
energy — and you get a two-dimensional picture instead of a single curve.
This is the **short-time Fourier transform** (STFT); the picture it
produces is a **spectrogram**:

\[
X(t, f) = \int x(\tau)\, w(\tau - t)\, e^{-j2\pi f \tau}\, d\tau
\]

`w(\tau - t)` is the window, large near \(\tau = t\) and fading to zero
away from it, sliding as `t` advances. Discretely this is nothing more
exotic than a windowed FFT computed once per hop across the recording.

The window length is not a free choice. Dennis Gabor showed, in 1946, that
time resolution and frequency resolution trade off against each other by a
fixed lower bound that no window shape escapes: a narrow window localizes
*when* well and *what* poorly; a wide window does the reverse.

Wavelets are the standard answer to wanting both at once — short windows
for high frequencies, which change quickly and need fine time resolution,
and long windows for low frequencies, which change slowly and need fine
frequency resolution instead. A single family of basis functions is
stretched and shifted rather than kept at one fixed width:

\[
W(a, b) = \frac{1}{\sqrt{|a|}} \int x(t)\, \psi\!\left(\frac{t - b}{a}\right) dt
\]

`a` scales the wavelet \(\psi\) (wide `a` for low frequencies, narrow `a`
for high ones) and `b` slides it along the signal, giving a
multi-resolution picture rather than one fixed window size.

## In Qu

Run a real sweep through the same whole-signal FFT to make the problem
concrete, then fix it:

```qu
fs = 200
n = 400
x = chirp(5, 45, fs, n)
whole = abs(rfft(x))
print("magnitude at 5, 15, 25, 35, 45 Hz: {whole[10]:.2f}, {whole[30]:.2f}, {whole[50]:.2f}, {whole[70]:.2f}, {whole[90]:.2f}")
```

```
magnitude at 5, 15, 25, 35, 45 Hz: 11.75, 23.49, 21.52, 23.66, 11.11
```

`chirp` sweeps linearly from 5 Hz to 45 Hz. All five frequencies show
substantial energy and none dominates — technically correct, since the
sweep passed through all of them, and useless for the question that
actually matters: in what order. Qu's `spectrogram` performs the
windowed-FFT framing directly and hands back the underlying data alongside
the plot it draws:

```qu
figure()
r = spectrogram(x, 64, 32, fs)
print("db shape = {shape(r.db)}")

peak_rows = argmax(r.db, axis = 0)
peak_freqs = r.freq[peak_rows]
print("peak_freqs = {peak_freqs:.2f}")
print("first frame: {peak_freqs[0]:.2f} Hz, last frame: {peak_freqs[length(peak_freqs) - 1]:.2f} Hz")
```

```
db shape = [33, 11]
peak_freqs = [9.375, 12.5, 15.625, 18.75, 21.875, 25, 28.125, 31.25, ... (11 elements)]
first frame: 9.38 Hz, last frame: 40.62 Hz
```

Eleven frames, each a 64-sample slice, and the loudest frequency per frame
climbs steadily from 9.4 Hz to 40.6 Hz — coarse, at 3.125 Hz per bin, but
tracking the sweep in order. This is what the whole-signal FFT could not
give at any resolution.

The window length here, 64 samples, was not free. Try both extremes on the
same chirp:

```qu
figure()
short = spectrogram(x, 16, 8, fs)
figure()
long  = spectrogram(x, 128, 64, fs)
print("short: {shape(short.db)}, resolution {short.freq[1] - short.freq[0]:.3f} Hz")
print("long:  {shape(long.db)}, resolution {long.freq[1] - long.freq[0]:.3f} Hz")

sp = short.freq[argmax(short.db, axis = 0)]
lp = long.freq[argmax(long.db, axis = 0)]
print("short-window peaks: {sp:.2f}")
print("long-window peaks: {lp:.2f}")
```

```
short: [9, 49], resolution 12.500 Hz
long:  [65, 5], resolution 1.562 Hz
short-window peaks: [0, 12.5, 0, 12.5, 12.5, 0, 0, 12.5, ... (49 elements)]
long-window peaks: [10.9375, 17.1875, 25, 31.25, 37.5]
```

The 16-sample window gives 49 time frames but only 12.5 Hz frequency
resolution — so coarse that the tracked peak jumps uselessly between 0 and
12.5 Hz instead of tracing a sweep. The 128-sample window gives a clean,
believable rise (10.9 to 37.5 Hz) at 1.6 Hz resolution, but only 5 frames
to describe 400 samples of sweep, throwing away the timing precision the
short window had. This is Gabor's bound, not a setting to tune away.

Qu's own wavelet support is real but narrow, and worth stating plainly:
`dwt`/`idwt` implement exactly one wavelet family, the Haar wavelet — the
simplest orthogonal wavelet there is, and the only one shipped so far.
Daubechies, Morlet, and the rest are not implemented; nothing quietly
stands in for them.

```qu
x = [1, 2, 3, 4]
D = dwt(x)
approx = D[0, :]
detail = D[1, :]
print("approx = {approx:.6f}")
print("detail = {detail:.6f}")

xr = idwt(D)
print("reconstructed = {xr}")
print("max reconstruction error = {max(abs(xr - x)):.2e}")
```

```
approx = [2.12132, 4.949747]
detail = [-0.707107, -0.707107]
reconstructed = [1, 2, 3, 4]
max reconstruction error = 8.88e-16
```

One level of Haar splits a length-4 signal into a length-2 "approx" band
(each pair's sum, scaled by \(1/\sqrt{2}\)) and a length-2 "detail" band
(each pair's difference) — orthogonal, so `idwt` reconstructs the original
exactly, to floating-point precision. Run it again on the approx band and
you get a coarser decomposition layered on top, the genuine
multi-resolution structure a fixed STFT window cannot give. What one
wavelet shape does not buy you is the smoothness and frequency selectivity
real audio and image work usually wants — Haar's sharp rectangular shape
shows up as blocky artifacts wherever a signal is smooth. Treat today's
`dwt` as a correct, honestly limited first tool, not a finished wavelet
library.

You can now see *what* is in a signal (Lesson 4), *whether a system stays
under control* (Lesson 5), and *when* content occurs (this lesson). None
of that has let you act — remove the hum, keep the speech, reject the
interference. Every transform so far has been a way of looking. Lesson 7
starts building the tools that reach into a signal and change it.
