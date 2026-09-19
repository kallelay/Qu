# 15. Coding and Compression

## The idea

Even a well-equalized channel occasionally hands back a wrong bit —
information genuinely destroyed in transit. Two kinds of deliberately
added redundancy deal with what a channel leaves behind, in opposite
directions:

- **Error-correcting codes** add structure a message didn't have, so
  damage in transit can be detected and reversed without a
  retransmission.
- **Compression** removes structure a message already had but didn't
  need, because real signals are far more predictable than a raw,
  sample-by-sample listing suggests.

**Coding.** Richard Hamming, working at Bell Labs in 1947 with a computer
that halted on any parity error and waited for a human to restart it,
reasoned that a machine which can already tell *that* an error occurred
has almost enough information to say *where*. Hamming(7,4), published in
1950, appends 3 parity bits to 4 data bits, each parity bit covering a
different subset of the 7 transmitted bits:

\[
p_1 = d_1 \oplus d_2 \oplus d_4, \qquad
p_2 = d_1 \oplus d_3 \oplus d_4, \qquad
p_4 = d_2 \oplus d_3 \oplus d_4
\]

sent as codeword \([p_1, p_2, d_1, p_4, d_2, d_3, d_4]\). At the receiver,
each parity check is recomputed; the pattern of *which* checks disagree
forms a 3-bit syndrome that names the exact bit position that flipped, or
zero if none did. Three extra bits buy the ability to survive any single
flip in seven — a 75% overhead that looks wasteful only to someone who
has not just lost a weekend of computation to one.

**Compression.** A smooth, structured signal doesn't need every sample
treated as independent information. Transform coding exploits this: pass
the signal through a basis, such as the Discrete Cosine Transform (DCT),
chosen so that most of the signal's energy lands in a handful of
low-frequency coefficients,

\[
X_k = \sum_{n=0}^{N-1} x_n \cos\!\left[\frac{\pi}{N}\left(n+\tfrac12\right)k\right],
\]

keep only the largest coefficients, and discard the rest. The idealized
picture — most of a transform's energy concentrated in the first few
coefficients, the remainder trailing off — looks like this for any
smooth, low-frequency-dominated signal:

```qu
k = 0 to 15
mag = 10 * exp(-0.5*k)
figure()
stem(k, mag)
xlabel("coefficient index k")
ylabel("|coefficient|")
title("Idealized transform-coefficient energy decay")
```

This is the actual mechanism behind JPEG (1992), which splits an image
into 8x8 blocks and quantizes each block's 2-D DCT most aggressively
where the eye is least sensitive, and MP3 (1993), which runs a close
relative, the Modified DCT, on overlapping windows of audio for the same
reason: concentrate the energy, then spend bits where the energy actually
is.

## In Qu

**Hamming(7,4) from bitwise primitives.** Qu has no built-in Hamming or
CRC codec — this is structure added by hand, from `bitxor`, `bitor`, and
`bitshift`:

```qu
function hamming_encode(d)
    d1 = d[0]
    d2 = d[1]
    d3 = d[2]
    d4 = d[3]
    p1 = bitxor(bitxor(d1, d2), d4)
    p2 = bitxor(bitxor(d1, d3), d4)
    p4 = bitxor(bitxor(d2, d3), d4)
    return [p1, p2, d1, p4, d2, d3, d4]
end function

d = [1, 0, 1, 1]
codeword = hamming_encode(d)
print("data = {d}, codeword = {codeword}")
```

```
data = [1, 0, 1, 1], codeword = [0, 1, 1, 0, 0, 1, 1]
```

`bitshift(s4, 2)` and `bitshift(s2, 1)` below pack the three syndrome bits
into one integer, the same trick as writing a decimal number digit by
digit, done with shifts instead of powers of ten:

```qu
function hamming_syndrome(c)
    s1 = bitxor(bitxor(bitxor(c[0], c[2]), c[4]), c[6])
    s2 = bitxor(bitxor(bitxor(c[1], c[2]), c[5]), c[6])
    s4 = bitxor(bitxor(bitxor(c[3], c[4]), c[5]), c[6])
    return bitor(bitshift(s4, 2), bitor(bitshift(s2, 1), s1))
end function

function hamming_correct(c)
    s = hamming_syndrome(c)
    fixed = c
    if s > 0 then
        fixed[s - 1] = bitxor(c[s - 1], 1)
    end if
    return fixed
end function

data_bits(c) := [c[2], c[4], c[5], c[6]]

total_ok = 0
for flip = 0 to 6
    corrupted = codeword
    corrupted[flip] = bitxor(corrupted[flip], 1)
    fixed = hamming_correct(corrupted)
    recovered = data_bits(fixed)
    ok = sum(abs(recovered - d)) == 0
    print("flip bit {flip}: syndrome = {hamming_syndrome(corrupted)}, recovered = {recovered}, correct = {ok}")
    if ok then
        total_ok += 1
    end if
end for
print("corrected {total_ok} / 7 single-bit errors")
```

```
flip bit 0: syndrome = 1, recovered = [1, 0, 1, 1], correct = true
flip bit 1: syndrome = 2, recovered = [1, 0, 1, 1], correct = true
flip bit 2: syndrome = 3, recovered = [1, 0, 1, 1], correct = true
flip bit 3: syndrome = 4, recovered = [1, 0, 1, 1], correct = true
flip bit 4: syndrome = 5, recovered = [1, 0, 1, 1], correct = true
flip bit 5: syndrome = 6, recovered = [1, 0, 1, 1], correct = true
flip bit 6: syndrome = 7, recovered = [1, 0, 1, 1], correct = true
corrected 7 / 7 single-bit errors
```

Every syndrome equals the 1-indexed position of the flipped bit, and all
seven possible single-bit errors are corrected exactly.

**DCT energy compaction, measured.** Two sinusoids and a trace of noise,
64 samples, run through Qu's `dct`:

```qu
seed(4)
n = 0 to 63
x = sin(2*pi*n/64) + 0.5*sin(2*pi*2*n/64) + 0.02*randn(64)
c = dct(x)
print("c[0:7] = {c[0:7]}")

energy_total = sum(c.^2)
energy_top8 = sum(c[0:7].^2)
print("energy fraction in first 8 of 64 coefficients = {100*energy_top8/energy_total:.2f}%")

figure()
stem(n, abs(c))
xlabel("DCT coefficient index")
ylabel("|coefficient|")
title("DCT coefficient magnitudes: energy concentrates at low index")
```

```
c[0:7] = [-0.02223, 5.747685, -0.27347, -0.836483, -0.294924, -2.229977, 0.018975, -0.71191]
energy fraction in first 8 of 64 coefficients = 99.17%
```

99.17% of the energy sits in the first 8 of 64 coefficients. Zero the
other 56 — an 8x reduction in storage — and reconstruct with `idct`:

```qu
c_trunc = c
for k = 8 to 63
    c_trunc[k] = 0
end for
xr = idct(c_trunc)
err = rms(xr - x) / rms(x)
print("relative reconstruction error, keeping 8/64 coefficients = {err:.4f}")

figure()
plot(n, x)
plot(n, xr)
xlabel("sample n")
ylabel("amplitude")
title("Original vs. reconstruction from 8/64 DCT coefficients")
legend("original x", "reconstructed (8 coeffs)")
```

```
relative reconstruction error, keeping 8/64 coefficients = 0.0914
```

9.14% relative error for an 87.5% cut in data is a genuine trade, not a
free lunch — and the error does not vanish with more coefficients kept,
because 0.02 of it is the injected noise itself, which the DCT correctly
refuses to compact. Noise has no structure to concentrate.

A Hamming code spends bits to buy resilience; a DCT truncation spends
accuracy to buy economy. Neither one asks what kind of signal it is
protecting or compressing — Hamming(7,4) corrects a flipped bit whether it
came from a voice call or a sensor log, and the DCT compacts energy
whether the samples came from a microphone or a photograph. Every signal
in this course so far has unfolded along one axis: time. An image is not
that — it carries two axes at once, and reasoning about it means asking
what convolution, sampling, and frequency mean when there is a "sideways"
as well as a "forward."

**Next: [Lesson 16 — Image and video processing](16-image-video-processing.md).**
