import numpy as np
import time

n_states = 5
n_symbols = 5
T = 8000


def make_row_stochastic(n_rows, n_cols):
    m = np.zeros((n_rows, n_cols))
    for i in range(n_rows):
        row = np.array([1.5 + np.sin(2 * i + 5 * j + 0.3) for j in range(n_cols)])
        m[i] = row / row.sum()
    return m


transition = make_row_stochastic(n_states, n_states)
emission = make_row_stochastic(n_states, n_symbols)
initial = np.full(n_states, 1.0 / n_states)

seed = 7
obs = np.empty(T, dtype=int)
for i in range(T):
    seed = (25173 * seed + 13849) % 65536
    obs[i] = seed % n_symbols

t0 = time.perf_counter()

log_trans = np.log(transition)
log_emit = np.log(emission)
log_init = np.log(initial)

delta = log_init + log_emit[:, obs[0]]
psi = np.zeros((T, n_states), dtype=int)

for t in range(1, T):
    scores = delta[:, None] + log_trans          # (n_states_prev, n_states_next)
    psi[t] = np.argmax(scores, axis=0)
    delta = scores[psi[t], np.arange(n_states)] + log_emit[:, obs[t]]

path = np.zeros(T, dtype=int)
path[-1] = int(np.argmax(delta))
for t in range(T - 2, -1, -1):
    path[t] = psi[t + 1, path[t + 1]]

elapsed_ms = (time.perf_counter() - t0) * 1000

print(f"[Python] path length={len(path)}")
print(f"[Python] first 20 states={path[:20].tolist()}")
print(f"[Python] checksum(sum of states)={int(path.sum())}")
print(f"[Python] elapsed={elapsed_ms:.2f} ms for {n_states} states x {T} steps")
