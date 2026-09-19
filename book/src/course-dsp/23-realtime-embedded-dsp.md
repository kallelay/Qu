# Lesson 23 — Real-Time Embedded DSP

Every signal so far in this course has been a finished array, sitting in
memory, waiting patiently to be transformed. A microphone does not work
that way. Samples arrive at a fixed rate whether or not the processor is
ready for the last one, forever, until the device is switched off.
Real-time DSP is not signal processing with a stopwatch attached — it is
signal processing where being right and being late are the same as being
wrong.

## The idea

An analog-to-digital converter delivers samples at a fixed rate \(f_s\).
Rather than react to every single sample the instant it lands, real
systems collect them into a **block** (or **buffer**) of \(N\) samples,
process the whole block at once, and hand it off — the standard trade
that lets a chip spend a little on scheduling overhead per samples rather
than per sample. That grouping fixes the system's **deadline**: the block
must be fully processed before the next one is complete, or

\[
\tau = \frac{N}{f_s}
\]

seconds have elapsed and the input has already moved on. Miss it once and
a **hard real-time** system (a pacemaker, a braking controller) has
failed outright; a **soft real-time** system (a phone call, a video call)
just produces a click or a dropped frame and continues. Neither kind
tolerates missing every deadline, only how expensive one miss is differs.

Production hardware hides the wait with **double buffering**: a DMA
controller silently fills buffer B with fresh samples via direct
memory access while the processor works through buffer A, then the roles
swap. The processor is never blocked waiting on the ADC; it is only ever
racing its own deadline, not the converter's clock. Whether that race is
won is exactly what the illustrations below make visible.

Smaller blocks make the deadline tighter but cut **latency** — the delay
between a sample arriving and its processed version leaving — since a
huge buffer cannot be handed off until it is completely full. Real
systems pick \(N\) as a compromise: small enough that a caller does not
hear a lag, large enough that per-block overhead does not eat the budget.

```qu
N = 256
block = 32
t = 0 to N - 1
x = sin(2 * pi * t / 40)
figure()
plot(t, x)
for b in 1 to (N / block) - 1
    vline(b * block, color="gray")
end for
xlabel("sample index n")
ylabel("x[n]")
title("A continuous stream cut into fixed-size buffers")
```

```qu
idx = 0 to 7
budget = [3, 4, 3.5, 4.2, 3.8, 6.1, 4.0, 3.6]
deadline_demo = 5
figure()
bar(idx, budget)
plot([-0.5, 7.5], [deadline_demo, deadline_demo], color="red", label="deadline")
xlabel("block index")
ylabel("processing time (arbitrary units)")
title("A missed deadline: one block ran over budget")
legend()
```

Seven blocks finish comfortably under budget; the sixth does not. Nothing
about that sixth block's *output* is wrong — it computed the right
numbers. It computed them too slowly, which in a real-time system is the
only kind of wrong that matters.

## In Qu

Qu has no interrupt handler and no DMA controller — it is a scripting
language, not a real-time OS. What it has, from the concurrency chapter
of the standard library, is the closest working analog: `timer()` for
lap-accurate wall-clock measurement, and `pmap`/`spawn`/`pool` for
spreading work across threads. That is enough to measure whether a piece
of DSP code *would* meet a deadline, even without the hardware that would
enforce one.

The example below filters an 8000 Hz stream through a 9-tap moving-average
filter, 64 samples at a time — a 64/8000 = 8 ms deadline per block — and
laps a `timer()` after every block to record how long that block actually
took:

```qu
seed(3)
fs = 8000
block_size = 64
deadline = block_size / fs
num_blocks = 40
N2 = block_size * num_blocks
t2 = (0 to N2 - 1) / fs
sig = sin(2 * pi * 300 * t2) + 0.3 * randn(N2, seed = 11)
h = ones(9) / 9

lap = timer()
times = zeros(num_blocks)
y = zeros(N2)
for b in 0 to num_blocks - 1
    i0 = b * block_size
    i1 = i0 + block_size - 1
    blk = sig[i0:i1]
    filtered = conv(blk, h, mode="same")
    y[i0:i1] = filtered
    times[b] = restart(lap)
end for

misses = length(where(times > deadline))
print("deadline per block (s): {round(deadline, 6)}")
print("mean block time (s): {round(mean(times), 8)}")
print("max block time (s): {round(max(times), 8)}")
print("deadline misses: {misses} of {num_blocks}")

figure()
plot(0 to num_blocks - 1, times * 1000, "o", label="block processing time")
plot([0, num_blocks - 1], [deadline * 1000, deadline * 1000], label="deadline")
xlabel("block index")
ylabel("time (ms)")
title("Per-block processing time vs. real-time deadline")
legend()
```

```
deadline per block (s): 0.008
mean block time (s): 4.64e-6
max block time (s): 3.8e-5
deadline misses: 0 of 40
```

Every block finishes roughly a thousand times faster than its budget on
this machine — a 9-tap filter over 64 samples is nothing for a modern
CPU, which is exactly why a phone call does not require exotic hardware
today. The margin is the point: real-time DSP design is the art of
knowing that margin precisely, not assuming it.

It is tempting to reach for `pmap` and spread the blocks across the
thread pool the concurrency chapter documents, since more cores sounds
like more headroom:

```qu
function block_rms(b)
    i0 = b * block_size
    i1 = i0 + block_size - 1
    blk = sig[i0:i1]
    return sqrt(mean(blk .^ 2))
end function

block_idx = 0 to num_blocks - 1
tic()
rms_pool = block_idx |> pmap("block_rms")
pool_time = toc()
tic()
rms_seq = zeros(num_blocks)
for b in block_idx
    rms_seq[b] = block_rms(b)
end for
seq_time = toc()

print("pool wall time (s): {round(pool_time, 6)}")
print("sequential wall time (s): {round(seq_time, 6)}")
print("results match: {max(abs(rms_pool - rms_seq)) < 1e-12}")
```

```
pool wall time (s): 0.001005
sequential wall time (s): 0.000179
results match: true
```

The parallel version is *slower* — roughly five times slower, on this
run — because dispatching forty tiny jobs onto `rayon`'s work-stealing
pool costs more than the 64-sample RMS calculation each job actually
does. This is a real, measured result, not a caveat added for balance:
parallelism has a fixed per-job overhead, and a real-time block small
enough to make its deadline in the first place is frequently too small
to be worth parallelizing at all. It also exposes why a work-stealing
pool is the wrong tool for hard real-time regardless of speed: nothing
about `pmap` bounds *worst-case* latency for any one job, which is what a
deadline actually demands, only average throughput across many.

Finally, splitting a filter across block boundaries is not free even when
timing is not the concern:

```qu
y_whole = conv(sig, h, mode="same")
zoom = 60 to 75
figure()
plot(zoom, y_whole[zoom], label="filtered whole signal")
plot(zoom, y[zoom], label="filtered block-by-block")
xlabel("sample index")
ylabel("amplitude")
title("Block-boundary seam near sample 64")
legend()

print("max diff at boundary window: {round(max(abs(y_whole[zoom] - y[zoom])), 4)}")
```

```
max diff at boundary window: 0.4507
```

Filtering block 0 and block 1 independently throws away the samples each
one would have needed from its neighbor, and the seam at sample 64 shows
it: a real, measurable discrepancy against filtering the same signal
whole. (Production systems fix this with overlap-add or overlap-save,
already covered in Lesson 8 — the point here is that block processing
reopens a problem the filter-design lessons had already closed, the
moment the block boundary is real rather than theoretical.)

None of this — a comfortable timing margin, a pool that does not help, a
seam that is fixable — says anything about whether the *scheduler*
underneath ever guarantees a worst case. `rayon`'s pool gives none: no
priority, no bounded latency, no promise that this measurement replays
identically under load. A real embedded system replaces it with a
real-time OS or a bare interrupt handler for exactly that reason. Lesson
24 assumes that guarantee already holds and builds on top of it: a radar
return that must be timestamped precisely, a biomedical measurement taken
on a fixed cadence, and a control loop that only stays stable if its
correction arrives on schedule, every time.
