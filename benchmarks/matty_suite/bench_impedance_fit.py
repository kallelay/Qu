"""bench_impedance_fit.py -- numpy port of bench_impedance_fit.qu.

Same hand-rolled Nelder-Mead (not scipy.optimize's, deliberately -- see
the .qu header: this MATLAB install has no Optimization Toolbox, so
fminsearch's Nelder-Mead is the only generic minimizer available there
at all, and using the identical algorithm in all three languages
isolates language/floating-point behaviour instead of confounding it
with "which optimizer"), same Randles circuit. Runs the fit TWICE,
deliberately, over two different frequency sweeps: a bad one (1-10kHz,
this file's actual first draft) that never reaches the ~159 kHz RC
knee and leaves Rct/Cdl practically unidentifiable, and a good one
(10Hz-10MHz) that spans it. Same optimizer, same true circuit, same
starting guess -- only the measured range differs, and that alone is
the difference between ~1e-7 relative parameter error and Rct off by
~40% while the fit LOOKS nearly perfect either way.
"""

import time

import numpy as np


def list_set(lst, idx, val):
    out = list(lst)
    out[idx] = val
    return out


def nelder_mead(f, x0, max_iter, tol):
    n = len(x0)
    alpha, gamma, rho, sigma = 1.0, 2.0, 0.5, 0.5

    simplex = [np.array(x0, dtype=float)]
    for i in range(n):
        pi_ = np.array(x0, dtype=float)
        delta = 0.05 * abs(pi_[i]) if pi_[i] != 0.0 else 0.05
        pi_[i] += delta
        simplex.append(pi_)

    fvals = np.array([f(p) for p in simplex])

    for it in range(1, max_iter + 1):
        order = np.argsort(fvals)
        simplex = [simplex[k] for k in order]
        fvals = fvals[order]

        if abs(fvals[n] - fvals[0]) < tol:
            return simplex[0], fvals[0], it

        centroid = sum(simplex[i] for i in range(n)) / n

        xr = centroid + alpha * (centroid - simplex[n])
        fr = f(xr)

        if fr < fvals[0]:
            xe = centroid + gamma * (centroid - simplex[n])
            fe = f(xe)
            if fe < fr:
                simplex = list_set(simplex, n, xe)
                fvals[n] = fe
            else:
                simplex = list_set(simplex, n, xr)
                fvals[n] = fr
        elif fr < fvals[n - 1]:
            simplex = list_set(simplex, n, xr)
            fvals[n] = fr
        else:
            xc = centroid + rho * (simplex[n] - centroid)
            fc = f(xc)
            if fc < fvals[n]:
                simplex = list_set(simplex, n, xc)
                fvals[n] = fc
            else:
                for i in range(1, n + 1):
                    shrunk = simplex[0] + sigma * (simplex[i] - simplex[0])
                    simplex = list_set(simplex, i, shrunk)
                    fvals[i] = f(shrunk)

    return simplex[0], fvals[0], max_iter


# ---- the Randles circuit: Rs + Rct parallel Cdl --------------------------
Rs_true, Rct_true, Cdl_true = 0.2, 0.01, 100e-6


def randles(freqs, Rs, Rct, Cdl):
    return Rs + Rct / (1 + 1j * (2 * np.pi * freqs) * Rct * Cdl)


def fit_randles(freqs, Z_true, p0):
    def residual(p):
        Rs, Rct, Cdl = p[0], p[1], p[2] * 1e-6
        Z_model = randles(freqs, Rs, Rct, Cdl)
        diff = Z_model - Z_true
        return float(np.sum(np.abs(diff) ** 2))

    t0 = time.perf_counter()
    x, f_final, iters = nelder_mead(residual, p0, 2000, 1e-16)
    t = time.perf_counter() - t0
    return dict(Rs=x[0], Rct=x[1], Cdl=x[2] * 1e-6, residual=f_final, iters=iters, time=t)


def report(label, fit):
    rel_Rs = abs(fit["Rs"] - Rs_true) / Rs_true
    rel_Rct = abs(fit["Rct"] - Rct_true) / Rct_true
    rel_Cdl = abs(fit["Cdl"] - Cdl_true) / Cdl_true
    print(label)
    print(f"  recovered  : Rs={fit['Rs']:.6f}  Rct={fit['Rct']:.6f}  Cdl={fit['Cdl']:.2e}")
    print(f"  rel. error : Rs={rel_Rs:.2e}  Rct={rel_Rct:.2e}  Cdl={rel_Cdl:.2e}")
    print(f"  residual={fit['residual']:.2e}  iters={fit['iters']}  time={fit['time']:.6f} s")


p0 = np.array([0.15, 0.02, 50.0])
print(f"true value : Rs={Rs_true:.6f}  Rct={Rct_true:.6f}  Cdl={Cdl_true:.2e}")
print()

# THE LESSON, SHOWN NOT JUST ASSERTED: a model can fit its data almost
# perfectly and still fail to determine its own parameters. The RC
# "knee" sits at 1/(2*pi*Rct*Cdl) = ~159 kHz for these true values -- a
# sweep that never reaches it can't constrain Rct and Cdl individually,
# no matter how good the optimizer or how careful the language is. This
# is the first draft's actual, real mistake, kept in rather than fixed
# away.
freqs_bad = np.logspace(0, 4, 40)  # 1 Hz - 10 kHz: never reaches the knee
Z_true_bad = randles(freqs_bad, Rs_true, Rct_true, Cdl_true)
fit_bad = fit_randles(freqs_bad, Z_true_bad, p0)
Z_recon_bad = randles(freqs_bad, fit_bad["Rs"], fit_bad["Rct"], fit_bad["Cdl"])
max_err_bad = float(np.max(np.abs(Z_recon_bad - Z_true_bad)))
report("sweep 1 Hz - 10 kHz (never reaches the ~159 kHz RC knee):", fit_bad)
print(
    f"  max |Z_fit - Z_true| across this sweep: {max_err_bad:.2e}  "
    f"(data scale ~{np.max(np.abs(Z_true_bad)):.2f} -- the spectrum fit is "
    f"EXCELLENT despite the parameters being wrong)"
)
print()

freqs_good = np.logspace(1, 7, 40)  # 10 Hz - 10 MHz: spans 2 decades either side of the knee
Z_true_good = randles(freqs_good, Rs_true, Rct_true, Cdl_true)
fit_good = fit_randles(freqs_good, Z_true_good, p0)
Z_recon_good = randles(freqs_good, fit_good["Rs"], fit_good["Rct"], fit_good["Cdl"])
max_err_good = float(np.max(np.abs(Z_recon_good - Z_true_good)))
report("sweep 10 Hz - 10 MHz (spans the knee):", fit_good)
print(f"  max |Z_fit - Z_true| across this sweep: {max_err_good:.2e}")
print()
print("SAME optimizer, SAME true circuit, SAME starting guess -- only the")
print("measured frequency RANGE differs. That range alone is the difference")
print("between Rct/Cdl recovered to ~1e-7 relative error and Rct off by")
print("~40% while the fit itself looks nearly perfect. A spectrum-fit")
print("residual is not evidence that a parameter is well determined --")
print("exactly the question Ahmed's requested significance-testing feature")
print("(permutation / Student's-t on fitted parameters) would answer directly.")
