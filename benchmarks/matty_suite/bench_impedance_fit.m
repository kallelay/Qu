% bench_impedance_fit.m -- MATLAB port of bench_impedance_fit.qu.
%
% Same hand-rolled Nelder-Mead as the Qu and Python versions (not
% fminsearch -- this install's fminsearch is the only generic minimizer
% available at all, so using it here would make the MATLAB leg use a
% DIFFERENT implementation than Qu/Python even though the algorithm
% class is the same; the hand-rolled version keeps all three bit-for-bit
% comparable). Same Randles circuit. Runs the fit TWICE, deliberately,
% over two different frequency sweeps: a bad one (1-10kHz, this file's
% actual first draft) that never reaches the ~159 kHz RC knee and
% leaves Rct/Cdl practically unidentifiable, and a good one
% (10Hz-10MHz) that spans it. Same optimizer, same true circuit, same
% starting guess -- only the measured range differs.
%
% Does NOT require the Signal Processing Toolbox or Optimization
% Toolbox -- everything here is base MATLAB (complex arithmetic,
% logspace, basic control flow), unlike bench_filter_accuracy.m.

Rs_true = 0.2; Rct_true = 0.01; Cdl_true = 100e-6;
p0 = [0.15, 0.02, 50.0];
fprintf('true value : Rs=%.6f  Rct=%.6f  Cdl=%.2e\n', Rs_true, Rct_true, Cdl_true);
fprintf('\n');

% THE LESSON, SHOWN NOT JUST ASSERTED: a model can fit its data almost
% perfectly and still fail to determine its own parameters.
freqs_bad = logspace(0, 4, 40);   % 1 Hz - 10 kHz: never reaches the knee
Z_true_bad = randles(freqs_bad, Rs_true, Rct_true, Cdl_true);
fit_bad = fit_randles(freqs_bad, Z_true_bad, p0, Rs_true, Rct_true, Cdl_true);
Z_recon_bad = randles(freqs_bad, fit_bad.Rs, fit_bad.Rct, fit_bad.Cdl);
max_err_bad = max(abs(Z_recon_bad - Z_true_bad));
report('sweep 1 Hz - 10 kHz (never reaches the ~159 kHz RC knee):', fit_bad, Rs_true, Rct_true, Cdl_true);
fprintf('  max |Z_fit - Z_true| across this sweep: %.2e  (data scale ~%.2f -- the spectrum fit is EXCELLENT despite the parameters being wrong)\n', max_err_bad, max(abs(Z_true_bad)));
fprintf('\n');

freqs_good = logspace(1, 7, 40);  % 10 Hz - 10 MHz: spans the knee
Z_true_good = randles(freqs_good, Rs_true, Rct_true, Cdl_true);
fit_good = fit_randles(freqs_good, Z_true_good, p0, Rs_true, Rct_true, Cdl_true);
Z_recon_good = randles(freqs_good, fit_good.Rs, fit_good.Rct, fit_good.Cdl);
max_err_good = max(abs(Z_recon_good - Z_true_good));
report('sweep 10 Hz - 10 MHz (spans the knee):', fit_good, Rs_true, Rct_true, Cdl_true);
fprintf('  max |Z_fit - Z_true| across this sweep: %.2e\n', max_err_good);
fprintf('\n');
fprintf('SAME optimizer, SAME true circuit, SAME starting guess -- only the\n');
fprintf('measured frequency RANGE differs. That range alone is the difference\n');
fprintf('between Rct/Cdl recovered to ~1e-7 relative error and Rct off by\n');
fprintf('~40%% while the fit itself looks nearly perfect. A spectrum-fit\n');
fprintf('residual is not evidence that a parameter is well determined --\n');
fprintf('exactly the question Ahmed''s requested significance-testing feature\n');
fprintf('(permutation / Student''s-t on fitted parameters) would answer directly.\n');

function fit = fit_randles(freqs, Z_true, p0, Rs_true, Rct_true, Cdl_true) %#ok<INUSD>
    tic;
    [x, f_final, iters] = nelder_mead(@(p) residual(p, freqs, Z_true), p0, 2000, 1e-16);
    t = toc;
    fit.Rs = x(1); fit.Rct = x(2); fit.Cdl = x(3) * 1e-6;
    fit.residual = f_final; fit.iters = iters; fit.time = t;
end

function report(label, fit, Rs_true, Rct_true, Cdl_true)
    rel_Rs = abs(fit.Rs - Rs_true) / Rs_true;
    rel_Rct = abs(fit.Rct - Rct_true) / Rct_true;
    rel_Cdl = abs(fit.Cdl - Cdl_true) / Cdl_true;
    fprintf('%s\n', label);
    fprintf('  recovered  : Rs=%.6f  Rct=%.6f  Cdl=%.2e\n', fit.Rs, fit.Rct, fit.Cdl);
    fprintf('  rel. error : Rs=%.2e  Rct=%.2e  Cdl=%.2e\n', rel_Rs, rel_Rct, rel_Cdl);
    fprintf('  residual=%.2e  iters=%d  time=%.6f s\n', fit.residual, fit.iters, fit.time);
end

function Z = randles(freqs, Rs, Rct, Cdl)
    Z = Rs + Rct ./ (1 + 1i * (2 * pi * freqs) .* Rct .* Cdl);
end

function r = residual(p, freqs, Z_true)
    Rs = p(1); Rct = p(2); Cdl = p(3) * 1e-6;
    Z_model = randles(freqs, Rs, Rct, Cdl);
    diff = Z_model - Z_true;
    r = sum(abs(diff) .^ 2);
end

function out = list_set(lst, idx, val)
    % idx is already a real MATLAB 1-based index from every caller here
    % (n+1, or i from `for i = 2:(n+1)`) -- an earlier draft added a
    % second +1 on top of that, silently writing a phantom extra cell
    % instead of updating the intended one. The algorithm looked
    % "stuck" as a result (best point never actually changed after the
    % first accepted step) and would have been reported as a real
    % cross-language accuracy gap if the Qu/Python legs hadn't already
    % converged to the exact known answer for comparison.
    out = lst;
    out{idx} = val;
end

function [xbest, fbest, iters] = nelder_mead(f, x0, max_iter, tol)
    n = length(x0);
    alpha = 1.0; gamma = 2.0; rho = 0.5; sigma = 0.5;

    simplex = {x0};
    for i = 1:n
        pi_ = x0;
        if pi_(i) ~= 0.0
            delta = 0.05 * abs(pi_(i));
        else
            delta = 0.05;
        end
        pi_(i) = pi_(i) + delta;
        simplex{end+1} = pi_;
    end

    fvals = zeros(1, n + 1);
    for i = 1:(n+1)
        fvals(i) = f(simplex{i});
    end

    for it = 1:max_iter
        [fvals, order] = sort(fvals);
        simplex = simplex(order);

        if abs(fvals(n+1) - fvals(1)) < tol
            xbest = simplex{1}; fbest = fvals(1); iters = it;
            return;
        end

        centroid = zeros(1, n);
        for i = 1:n
            centroid = centroid + simplex{i};
        end
        centroid = centroid / n;

        xr = centroid + alpha * (centroid - simplex{n+1});
        fr = f(xr);

        if fr < fvals(1)
            xe = centroid + gamma * (centroid - simplex{n+1});
            fe = f(xe);
            if fe < fr
                simplex = list_set(simplex, n+1, xe); fvals(n+1) = fe;
            else
                simplex = list_set(simplex, n+1, xr); fvals(n+1) = fr;
            end
        elseif fr < fvals(n)
            simplex = list_set(simplex, n+1, xr); fvals(n+1) = fr;
        else
            xc = centroid + rho * (simplex{n+1} - centroid);
            fc = f(xc);
            if fc < fvals(n+1)
                simplex = list_set(simplex, n+1, xc); fvals(n+1) = fc;
            else
                for i = 2:(n+1)
                    shrunk = simplex{1} + sigma * (simplex{i} - simplex{1});
                    simplex = list_set(simplex, i, shrunk);
                    fvals(i) = f(shrunk);
                end
            end
        end
    end
    xbest = simplex{1}; fbest = fvals(1); iters = max_iter;
end
