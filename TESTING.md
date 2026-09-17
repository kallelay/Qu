# Qu testing and performance policy

Qu numerical features are accepted only when correctness, determinism, CPU
performance, and accelerator parity are tested independently. A fast wrong
kernel and a correct kernel that silently falls back to the CPU both fail this
policy.

## Run the complete local gate

```powershell
.\scripts\test-signal.ps1
```

On a GPU development machine or hardware CI runner, require a real adapter:

```powershell
.\scripts\test-signal.ps1 -RequireGpu
```

The script builds into the operating-system temporary directory. This avoids
Dropbox/OneDrive archive-lock races and keeps concurrent Cargo jobs isolated.

## Test ladder

1. **Semantic regression tests** encode previously observed defects: global and
   local Mage targets differ; crossover consumes both parents; eliminated
   players cannot be reset back to life; matrix truthiness and `axis=` cover
   every runtime value.
2. **Differential tests** compare the accelerated recurrence kernel against a
   direct cosine oracle over 64 deterministic randomized shapes and phase sets.
3. **DSP invariants** verify DFT-bin snapping, negligible off-bin leakage,
   crest-factor scale invariance, deterministic synthesis, peak normalization,
   bounded DAC codes, and big-endian `float64` export.
4. **Advanced mathematical evals** compare radix-2 FFT with an independent
   naive DFT; enforce Parseval, modulation-shift, and inverse identities; check
   all four Moore-Penrose pseudoinverse laws on rectangular, rank-deficient,
   and ill-conditioned matrices; verify least-squares residual orthogonality;
   and compare the RPG L2p analytic Jacobian with central finite differences.
5. **CPU performance gate** runs the battery workload (15 tones, 2048 samples)
   in release mode. The recurrence kernel must beat the direct oracle and stay
   below 1 ms per waveform on the development baseline; debug mode has a 20 ms
   smoke-test budget.
6. **GPU parity and performance gate** synthesizes a full 80-player RPG
   population in one WebGPU dispatch, compares selected players with the CPU
   oracle, tests phase periodicity and tone-permutation invariance, sweeps
   irregular workgroup tails and matrix shapes, repeats the dispatch to test
   determinism, and requires completion below 250 ms. Hardware CI uses
   `-RequireGpu`; adapterless CI reports a skip.
7. **Native/WASM boundary tests** cross FFT, selection, mesh/hull arrays,
   Moore-Penrose pseudoinverse diagnostics, and minimum-norm least squares
   through the browser ABI, then compile that ABI for `wasm32-unknown-unknown`.
8. **Language corpus and integration tests** lex the complete RPGx Qu program,
   run the native reference-engine workspace, and run browser/documentation
   acceptance tests, including browser/native `**` precedence parity.

## Current measured baseline

Measured through 2026-08-22:

- CPU optimized multisine: **0.107-0.21 ms/waveform**, **3.16-5.44x** faster
  than the direct `cos` oracle across the measured runs.
- NVIDIA GeForce RTX 5080, Vulkan: **0.159-1.45 ms per 80-waveform population**,
  or **1.98-18.2 microseconds/waveform**, including readback.

These numbers are diagnostic baselines, not universal promises. The committed
budgets are intentionally looser to tolerate CI variance while still catching
algorithmic regressions and accidental CPU fallback.

## Required future suites

Each new numerical backend must add the same oracle/parity tests. The RPGx
optimizer itself still needs golden MATLAB vectors and statistical multi-seed
quality tests once its full Qu runtime path executes; those tests must compare
crest-factor distributions, convergence histories, and phase-equivalent output,
not demand bit identity from stochastic optimization.
