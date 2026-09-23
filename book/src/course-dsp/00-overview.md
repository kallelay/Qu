# Digital Signal Processing

**A course in 24 lessons. Start here.**

---

The last time you heard someone's voice on the phone, you did not hear
their voice.

What reached your ear was a reconstruction. Somewhere in the first
millisecond, a codec decided your friend's vocal tract could be described
by about ten numbers, refreshed fifty times a second, and threw away the
waveform itself. It sent the description. The handset in your hand built a
new voice from that description — never the original, close enough that
you recognised who was speaking and heard that they were tired.

That is digital signal processing. Not a subject that gets applied to the
world afterwards; the thing the world is already made of.

## You already own the instrument

Put two fingers on the inside of your wrist and wait.

What you feel is a signal: a value that changes with time and carries
information. It is nearly periodic, which means it has a frequency — call
it 70 beats per minute, 1.2 Hz. But it is not exactly periodic, and the
inexactness is the interesting part. The gaps between beats lengthen when
you exhale and shorten when you inhale. Take those gaps, transform them,
and a peak appears around 0.25 Hz that is your breathing, written into
your pulse without your consent or knowledge. Below it, slower structure
belonging to blood-pressure regulation. A clinician reads that plot and
learns something about your nervous system that no single heartbeat
contains.

You did not gain new data. The data was always there, in a form where it
could not be seen. Changing the form is the whole trick, and it is
available to you at the price of learning what a transform is.

That is the pattern this course teaches, over and over, in increasing
depth: a signal arrives in the shape the physics happened to deliver it,
and it is unreadable in that shape. You change the representation. The
answer was already in the data.

## Why bother mastering it

Plenty of skills are worth having. This one is worth more than it looks,
for three reasons.

**It transfers.** The mathematics that separates a tone from noise in an
audio recording separates a planet from starlight, a tumour boundary from
tissue, a fatigue crack from machine hum, a heartbeat from a radar return
through a wall. Not analogous mathematics — the same functions, called
with different numbers. You learn convolution once. You spend the rest of
your career recognising it in disguise.

**It is falsifiable.** A filter either removed the interference or it did
not, and the residual says which. This is a field where you can be
straightforwardly wrong and find out in a second, which is rarer and more
valuable than it sounds. Most of what you will believe about a signal can
be checked before you stake anything on it.

**It is old enough to be deep and young enough to be unfinished.** Fourier
published in 1822. Cooley and Tukey made his transform fast in 1965 and
changed what computers were for. Compressed sensing is younger than some
people reading this, and it says something Nyquist would have called
impossible. You are not arriving at a closed subject.

There is also the less defensible reason, which is the real one. Signals
are beautiful. A spectrogram of a wren's song looks like handwriting. A
filter's pole-zero plot tells you how it will sound before you hear it. At
some point you stop seeing a plot of numbers and start seeing the
mechanism that produced them, and that shift does not reverse.

## What this course does differently

Most DSP teaching separates the idea from the machine. You derive
something on a board on Tuesday, and on Thursday you fight an unrelated
toolchain until it plots. The gap between those days is where people quit,
and the quitting is usually blamed on the mathematics, which was not the
problem.

Here the theory and the running code are the same paragraph.

Every lesson states an idea in plain terms, writes it as an equation you
can read, and then does it — in Qu, in a few lines, with printed output
you can reproduce on your own machine in under a second. No pseudo-code.
No "an implementation is left to the reader." No screenshots of some other
program. If a lesson claims a filter has 40 dB of stopband rejection, the
lesson measures 40 dB in front of you, and if the number comes out at 38
the lesson says so and explains where the other 2 went.

You need no prior DSP. You need comfort with algebra, a rough memory of
what sine and cosine do, and a working Qu install. Complex numbers get
introduced properly when they are needed, in Lesson 4, as the tool they
actually are rather than as a hazing ritual.

## The map

Eight modules, twenty-four lessons. Read them in order the first time —
each module is standing on the one before it.

**Module 1 — Foundations.** What a signal *is*, and the one property that
makes a system tractable. *(1) Signal classification. (2) LTI systems and
convolution. (3) Sampling and quantization.* By the end you will know why
`y[n] = Σ_k h[k] · x[n-k]` describes almost every physical system you will
meet, and why a wheel on film sometimes turns backwards.

**Module 2 — Transforms and Spectra.** The change of representation that
makes the invisible visible. *(4) Fourier, DFT, FFT. (5) Laplace and
Z-transforms. (6) Time-frequency and wavelets.* This is the module people
remember. It contains the single most consequential algorithm in
engineering and the reason your music streams at all.

**Module 3 — Filters.** Deciding what to keep. *(7) Analog and digital
filters. (8) FIR and IIR design. (9) Adaptive filtering.* Ending with
filters that tune themselves against an interference they were never told
about — the mathematics inside every set of noise-cancelling headphones.

**Module 4 — Random Signals.** What to do when the signal will not repeat.
*(10) Random processes and PSD. (11) Detection and estimation. (12) Wiener
and Kalman filters.* Estimation under noise is where DSP stops being
arithmetic and starts being inference. The Kalman filter flew to the Moon.

**Module 5 — Communications.** Getting a signal across a hostile gap.
*(13) Modulation and demodulation. (14) Channels and equalization.
(15) Coding and compression.* Including the result that says exactly how
much information a noisy channel can carry, which is one of the few places
engineering has a hard physical limit and knows its value.

**Module 6 — Multidimensional DSP.** Signals with more than one axis.
*(16) Image and video processing. (17) Speech and audio. (18) Array and
spatial processing.* A photograph is a signal in two dimensions, and
sharpening it is a filter. Point enough microphones at a room and you can
steer your hearing without moving anything.

**Module 7 — Advanced Methods.** *(19) Multirate DSP and filter banks.
(20) Sparse and compressed sensing. (21) Machine learning for signals.*
Lesson 20 reconstructs a signal from far fewer samples than the sampling
theorem appears to permit. It is not a violation. It is a better question.

**Module 8 — Implementation and Applications.** Where the mathematics
meets a finite machine. *(22) DSP hardware and fixed point. (23) Real-time
embedded DSP. (24) Radar, biomedical, and control.* A filter that is
correct in infinite precision and unstable in 16 bits is a bug with a
specific name, and you will learn to see it coming.

That is the mountain. It is climbable in order, one lesson at a sitting.

## A taste, before you go

You do not have to wait for Lesson 4 to see the central move of this
entire subject. Three sounds played together — a note and two of its
overtones, the recipe behind why a violin and a flute playing the same
note are not the same sound:

```qu
fs = 8000                                  # 8 kHz sample rate
t  = (0 to 7999) / fs                      # one second of time
y  = sin(2*pi*440*t) + 0.5*sin(2*pi*880*t) + 0.25*sin(2*pi*1320*t)
```

As a list of 8000 numbers, `y` tells you nothing. It is a wobble. Now
change the representation:

```qu
mag  = abs(rfft(y))                        # the spectrum
f    = (0 to len(mag) - 1) * fs / len(y)   # what each bin means, in Hz
peak = argmax(mag)
print("strongest component: {f[peak]:.1f} Hz")
```

```
strongest component: 440.0 Hz
```

Three ingredients went in. `rfft` found them, sorted by strength, without
being told they were there, without being told anything about music. Plot
`mag` against `f` and the three spikes stand alone at 440, 880 and 1320 Hz
with nothing in between — the recipe recovered from the cake.

That is a one-second demonstration of the idea that took humanity until
1822 to state and until 1965 to make fast. Lesson 4 explains why it works.
Lesson 1 starts three steps before it, with a question worth more than it
first appears: what kind of signal is this, actually?

**Next: [Lesson 1 — Signal classification](01-signal-classification.md).**
Open a terminal first. This course is not a spectator sport.

## Exercises

1. Change the three-tone mixture's amplitudes to `1, 1, 1` (equal
   strength) instead of `1, 0.5, 0.25`, keep the frequencies, and re-run
   `argmax(mag)`. Does the strongest component change?
   **Check:** `argmax` breaks ties by returning the first match, so it
   should still report 440 Hz — the frequencies didn't move, only the
   heights did, and 440 is still first in the bin order.

2. Add a fourth tone at 2000 Hz to the mixture. Before running anything,
   predict whether `rfft` will find it as cleanly as the first three.
   **Check:** it should — nothing about the method depends on the tones
   being harmonics of 440 Hz. Confirm by checking that `mag` has a fifth
   clean spike near bin `2000/fs*len(y)`, same as the other three.

3. The wrist-pulse example earlier in this lesson describes finding a
   ~0.25 Hz breathing peak buried in heartbeat timing, by transforming a
   list of inter-beat gaps rather than the raw pulse waveform. Using only
   what this lesson showed (`rfft`, `argmax`, and the frequency-axis
   formula `f = (0 to len(mag)-1) * fs / len(y)`), sketch in words what
   `fs` would even mean for a signal built from gap durations rather than
   evenly-sampled time. **Check:** there is no natural sample rate for an
   unevenly-timed list of gaps — this is the actual reason "resample the
   gaps onto an even time grid first" is a real, necessary step, not a
   convenience. If your sketch didn't run into that problem, look again
   at what `fs` is doing in the frequency-axis formula.
