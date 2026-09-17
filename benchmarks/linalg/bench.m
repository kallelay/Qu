% bench.m -- MATLAB port of bench.qu/bench.py, same sizes/operations.
%
% Also runnable unmodified under matty's own interpreter:
%   python matty_runner.py <path to this file>   (from the matty/ repo root)
%
% Run: matlab -batch "run('benchmarks/linalg/bench.m')"

rng(42);
sizes = [100, 500, 1000];
for idx = 1:numel(sizes)
    run_size(sizes(idx));
end

function run_size(n)
    M = randn(n, n);
    A = M + M';

    tic; [V, D] = eig(A); t_eig = toc;
    eig_resid = norm(A*V - V*D, 'fro');

    tic; [Q, R] = qr(A); t_qr = toc;
    qr_resid = norm(A - Q*R, 'fro');

    tic; [L, U, P] = lu(A); t_lu = toc;
    lu_resid = norm(P*A - L*U, 'fro');

    tic; d = det(A); t_det = toc;

    tic; r = rank(A); t_rank = toc;

    tic; [U2, S, V2] = svd(A); t_svd = toc;
    svd_resid = norm(A - U2*S*V2', 'fro');

    fprintf('n=%d\n', n);
    fprintf('  eig  : %.4f s   resid=%.3e\n', t_eig, eig_resid);
    fprintf('  qr   : %.4f s   resid=%.3e\n', t_qr, qr_resid);
    fprintf('  lu   : %.4f s   resid=%.3e\n', t_lu, lu_resid);
    fprintf('  det  : %.4f s   d=%.4e\n', t_det, d);
    fprintf('  rank : %.4f s   r=%d\n', t_rank, r);
    fprintf('  svd  : %.4f s   resid=%.3e\n', t_svd, svd_resid);
end
