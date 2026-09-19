# 17. Speech & Audio Processing

A vowel's pitch comes from how fast the vocal folds buzz; the vowel itself comes from a resonant cavity, the vocal tract, reshaping that buzz on its way out.

## The idea

**The source-filter model.** Speech treats voice production as an ordinary LTI system: a fast, roughly periodic source \(s(t)\) (the vocal folds' buzz, or turbulent noise for unvoiced sounds) passed through a slower-changing filter \(h(t)\) (the vocal tract's current shape):

\[
y(t) = h(t) * s(t)
\]

The peaks in that filter's frequency response are the **formants**, conventionally numbered \(F_1, F_2, F_3\) from lowest to highest. They are essentially the whole reason one vowel is distinguishable from another regardless of who is speaking or at what pitch: change the tract's shape, move the peaks, hear a different vowel.

**Short-time stationarity.** A single Fourier transform over a whole recording assumes the system stayed fixed for its entire duration. Speech breaks that assumption every 20 to 40 milliseconds, as the vocal tract reshapes itself for the next phoneme — a global FFT would average dozens of different filters together and report almost nothing usable. The weaker, still useful claim is that *within* any one such window the filter barely moves, so an ordinary FFT taken there is meaningful. The **short-time Fourier transform** formalizes this: slide a window of length \(N\) by a hop of \(H\) samples and take a fresh FFT in each position,

\[
X[k,m] = \sum_{n=0}^{N-1} x[n + mH]\, w[n]\, e^{-j 2\pi kn/N}
\]

trading one global spectrum for a sequence of local ones. \(|X[k,m]|\) plotted against frame \(m\) and bin \(k\) is a **spectrogram** — a picture of a signal's frequency content changing over time.

```qu
seed(2)
Fs = 4000
N = 2000
n = 0 to N - 1
t = n / Fs
x1 = sin(2*pi*300*t[0:999])
x2 = sin(2*pi*900*t[1000:1999])
x = cat(1, x1, x2)

X = rfft(x)
mag = abs(X)
f = (0 to length(mag) - 1) * Fs / N
figure()
plot(f, mag)
title("One FFT over the whole record")
xlabel("frequency (Hz)")
ylabel("magnitude")
```

```qu
figure()
spectrogram(x, 256, 32, Fs)
title("Short-time Fourier transform of the same signal")
xlabel("time (s)")
ylabel("frequency (Hz)")
```

A tone that switches from 300 Hz to 900 Hz halfway through produces one FFT with two smeared peaks and no timing information at all; the STFT of the identical signal shows the switch, and exactly when it happens.

## In Qu

Build a toy vowel: three sustained tones standing in for three formants, sounding for 200 milliseconds inside half a second of noisy near-silence. First find *when* it happens using short-time energy, then read off *what* frequencies it carries from the STFT frame at that moment.

```qu
seed(3)
Fs = 8000
N = 4000
n = 0 to N - 1
t = n / Fs

active = abs(t - 0.25) <= 0.10
env = where(active, 1, 0)
voice = sin(2*pi*700*t) + 0.6*sin(2*pi*1200*t) + 0.3*sin(2*pi*2600*t)
x = env .* voice + 0.02 * randn(N)

win = 256
hop = 64
figure()
spectrogram(x, win, hop, Fs)
title("A synthetic 200 ms 'vowel'")
xlabel("time (s)")
ylabel("frequency (Hz)")

e = ste(x, win, hop)
voiced_frame = argmax(e)
print("voiced frame: {voiced_frame} at t = {voiced_frame * hop / Fs:.3f} s")

S = stft(x, win, hop)
col = abs(S[0:win/2, voiced_frame])
f = (0 to length(col) - 1) * Fs / win

peaks = find_peaks(col, min_height=max(col)*0.2, min_distance=3)
print("formant bins: {peaks}")
print("formant frequencies (Hz): {f[peaks]}")
```

```
voiced frame: 27 at t = 0.216 s
formant bins: [22, 38, 83]
formant frequencies (Hz): [687.5, 1187.5, 2593.75]
```

`ste` slides a 256-sample window across `x` and reports the energy in each one; the frame with the most energy lands at t = 0.216 s, comfortably inside the true 0.15-to-0.35 s voiced region, found without ever telling the code where that region was. That frame's STFT column, run through `find_peaks`, recovers three peaks at 687.5, 1187.5, and 2593.75 Hz against true formants of 700, 1200, and 2600 Hz — every one within 13 Hz. That gap is not measurement noise; it is the FFT's frequency grid. At `nfft = 256` and 8000 Hz, each bin covers `8000/256 = 31.25` Hz, so no reading can land closer than half a bin, 15.6 Hz, to the truth.

`ste` found the voiced region to within one 64-sample hop — plenty of precision for reading formants, not enough to pin an onset. The Teager-Kaiser energy operator tracks energy sample by sample instead of window by window, with no averaging to smear a sharp transition:

\[
\psi[x[n]] = x[n]^2 - x[n-1]\,x[n+1]
\]

```qu
psi = tkeo(x)
thresh = max(psi) * 0.1
onset_sample = find(psi > thresh)[0]
print("tkeo onset: sample {onset_sample}, t = {onset_sample / Fs:.4f} s (true onset: 0.1500 s)")
```

```
tkeo onset: sample 1200, t = 0.1500 s (true onset: 0.1500 s)
```

That lands on the true onset exactly, because this synthetic vowel switches on like a light switch; a real recording's onset is gradual and `tkeo` would show a ramp rather than a step, but the mechanism is the one a real onset detector uses: track something nonlinear in the energy, threshold it, take the first crossing. Qu has no dedicated formant tracker, no LPC, and no phoneme classifier — searching the signal-processing chapter for anything speech-specific turns up exactly `stft` and `find_peaks`, used above. That absence is not a hole so much as a fact worth stating: this *is* how a formant tracker starts. It is also, essentially, what Homer Dudley's vocoder did at Bell Labs in 1939, analysing a voice into a handful of resonance bands and re-synthesising speech from that description, decades before anyone had a Fourier transform fast enough to do it digitally.

## What one microphone cannot tell you

Every number above came from a single channel: one microphone, one waveform, one STFT. It could say *when* the vowel started and *what* frequencies it carried, but it had no way to answer a question just as basic — *where* did the sound come from? A single sensor collapses every direction in space onto the same one-dimensional trace, no matter how cleverly you window or transform it. The fix is not a better transform; it is more microphones. Lesson 18 turns space itself into an axis you can process.
