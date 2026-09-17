% bench_matty.m -- same operations/sizes as bench.m, for matty's own interpreter.
%
% Two real incompatibilities found running bench.m unmodified under matty
% (`python matty_runner.py bench.m`), both worked around here rather than
% silently skipped:
%   1. No `rng`/seeded-RNG support at all (checked `src/builtins.py`:
%      `randn` is a bare `np.random.randn(...)`, no seed argument accepted
%      anywhere) -- the `rng(42)` call is just dropped. Not a correctness
%      problem for this benchmark: timings were never meant to be
%      cross-language bit-comparable (each engine's `randn` draws different
%      numbers under a different seed or no seed at all), only
%      shapes/timing/per-language residual-correctness are.
%   2. No local/sub-function support in a script file -- real MATLAB (since
%      R2016b) allows a plain script to define helper functions after the
%      top-level code; matty's parser doesn't recognize them at all
%      ("Undefined function or variable 'run_size'" when bench.m's
%      `function run_size(n) ... end` was called). Every example shipped in
%      matty's own `examples/` is a flat script with no function
%      definitions, consistent with this being an actual gap, not a
%      one-off bug. Worked around by inlining the loop body directly
%      instead of factoring it into a callable function.
%
% A THIRD thing found, not a bench-script incompatibility but a genuine
% matty correctness bug relative to real MATLAB, confirmed by reading
% `src/builtins.py::f_lu`: matty's 3-output `[L,U,P] = lu(A)` forwards
% `scipy.linalg.lu(A)`'s own `(P, L, U)` tuple unchanged, which satisfies
% `A = P*L*U` (scipy's documented convention) -- but real MATLAB's
% `[L,U,P] = lu(A)` guarantees the OPPOSITE convention, `P*A = L*U`
% (confirmed against the real MATLAB R2025b run in bench.m/README.md).
% Checking matty's output against the real-MATLAB formula it's supposed to
% match (`P*A - L*U`) gives a huge, obviously-wrong residual (thousands, not
% ~1e-12); checking it against scipy's actual formula (`A - P*L*U`, used
% below, `lu_resid_scipy_convention`) confirms the underlying factorization
% itself is numerically correct -- matty just never adapted scipy's P/L/U
% ordering convention to MATLAB's, a real, confirmed API-compatibility bug
% for any Matty script relying on `[L,U,P]=lu(A)`'s documented MATLAB
% semantics.
%
% Run (from the matty/ repo root): python matty_runner.py <path to this file>

sizes = [100, 500, 1000];
for idx = 1:numel(sizes)
    n = sizes(idx);

    M = randn(n, n);
    A = M + M';

    tic; [V, D] = eig(A); t_eig = toc;
    eig_resid = norm(A*V - V*D, 'fro');

    tic; [Q, R] = qr(A); t_qr = toc;
    qr_resid = norm(A - Q*R, 'fro');

    tic; [L, U, P] = lu(A); t_lu = toc;
    lu_resid_matlab_convention = norm(P*A - L*U, 'fro');
    lu_resid_scipy_convention = norm(A - P*L*U, 'fro');
    % Third hypothesis: `src/builtins.py::f_lu` returns the raw
    % `scipy.linalg.lu(A)` tuple `(p, l, u)` positionally, and matty's
    % multi-output assignment binds return-tuple position i to the i-th
    % left-hand variable regardless of its name -- so `[L, U, P] = lu(A)`
    % actually receives L<-p, U<-l, P<-u (a silent swap relative to what the
    % names promise). If true, `A = p @ l @ u` becomes, in this script's own
    % (mis-bound) variable names, `A = L*U*P`.
    lu_resid_positional_swap = norm(A - L*U*P, 'fro');

    tic; d = det(A); t_det = toc;

    tic; r = rank(A); t_rank = toc;

    tic; [U2, S, V2] = svd(A); t_svd = toc;
    svd_resid = norm(A - U2*S*V2', 'fro');

    fprintf('n=%d\n', n);
    fprintf('  eig  : %.4f s   resid=%.3e\n', t_eig, eig_resid);
    fprintf('  qr   : %.4f s   resid=%.3e\n', t_qr, qr_resid);
    fprintf('  lu   : %.4f s   resid(MATLAB conv. P*A=L*U)=%.3e   resid(scipy conv. A=P*L*U)=%.3e   resid(positional-swap A=L*U*P)=%.3e\n', t_lu, lu_resid_matlab_convention, lu_resid_scipy_convention, lu_resid_positional_swap);
    fprintf('  det  : %.4f s   d=%.4e\n', t_det, d);
    fprintf('  rank : %.4f s   r=%d\n', t_rank, r);
    fprintf('  svd  : %.4f s   resid=%.3e\n', t_svd, svd_resid);
end
