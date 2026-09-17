import numpy as np
import time

np.random.seed(3)
N = 500
dt = 0.05
true_v = 1.5
t = np.arange(N) * dt
true_pos = true_v * t
measured = true_pos + 0.4 * np.random.randn(N)

N_PARTICLES = 50000
process_noise = 0.08
obs_noise = 0.4
spread = 2.0

rng = np.random.default_rng(3)


def systematic_resample(weights, rng):
    n = len(weights)
    positions = (rng.random() + np.arange(n)) / n
    cumsum = np.cumsum(weights)
    cumsum[-1] = 1.0
    return np.searchsorted(cumsum, positions)


t0 = time.perf_counter()

particles = np.empty((N_PARTICLES, 2))
particles[:, 0] = rng.normal(0.0, spread, N_PARTICLES)
particles[:, 1] = rng.normal(0.0, spread, N_PARTICLES)
weights = np.full(N_PARTICLES, 1.0 / N_PARTICLES)

est_pos = np.zeros(N)
est_vel = np.zeros(N)

for k in range(N):
    # predict (constant-velocity move + process noise)
    particles[:, 0] += particles[:, 1] * dt
    particles += rng.normal(0.0, process_noise, particles.shape)

    # update (Gaussian likelihood on position, ESS-gated systematic resample)
    likelihood = np.exp(-0.5 * ((particles[:, 0] - measured[k]) / obs_noise) ** 2)
    weights = weights * likelihood + 1e-300
    weights /= weights.sum()

    ess = 1.0 / np.sum(weights ** 2)
    if ess < N_PARTICLES / 2:
        idx = systematic_resample(weights, rng)
        particles = particles[idx]
        weights = np.full(N_PARTICLES, 1.0 / N_PARTICLES)

    est_pos[k] = np.sum(weights * particles[:, 0])
    est_vel[k] = np.sum(weights * particles[:, 1])

elapsed_ms = (time.perf_counter() - t0) * 1000

raw_err = np.sqrt(np.mean((measured - true_pos) ** 2))
filt_err = np.sqrt(np.mean((est_pos - true_pos) ** 2))
print(f"[Python] Position RMS error: raw sensor={raw_err:.3f}, particle filter estimate={filt_err:.3f}")
print(f"[Python] True velocity={true_v:.2f}, final estimated velocity={est_vel[-1]:.3f}")
print(f"[Python] elapsed={elapsed_ms:.1f} ms for {N_PARTICLES} particles x {N} steps")
