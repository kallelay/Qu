% fluid_simulation.m — same 2D diffusion (heat) equation, explicit FTCS
% scheme, IDENTICAL grid/parameters/initial-boundary conditions as
% catalog/qu_fluid_simulation.qu and fluid_simulation.py, so all three
% results are directly comparable.
%
% Like fluid_simulation.py, the Laplacian stencil update is vectorized
% array slicing (no scalar loop over grid cells) -- MATLAB's own idiomatic
% way to write this, and the natural third point of comparison against
% Qu's nested for-loop stencil and NumPy's vectorized slicing.
%
% Run non-interactively: matlab -batch "run('fluid_simulation.m')"
% Memory: MATLAB has no per-process RSS sampler exposed to script code, so
% this uses the built-in `memory` function's MemUsedMATLAB field (Windows
% only), sampled before/during/after, and reports the peak -- the same
% "peak whole-process memory" quantity Qu's --profile and the Python
% psutil sampler report, just read a different way. NOTE (see README):
% MATLAB's baseline process footprint is roughly 1.3-1.5 GB before this
% script allocates anything, which dwarfs the few-MB working set the
% actual 91x91 arrays need -- that baseline, not the simulation, dominates
% the MATLAB memory number below.

peak_mem_bytes = 0;
m0 = memory();
peak_mem_bytes = max(peak_mem_bytes, m0.MemUsedMATLAB);

% --- Parameters (identical to qu_fluid_simulation.qu / fluid_simulation.py)
Lx = 3.0;
Ly = 3.0;
nx = 91;
ny = 91;
D = 0.02;
t_final = 1.0;
cfl_factor = 0.4;

x0 = 1.5;
y0 = 1.5;
sigma0 = 0.15;
A0 = 1.0;

dx = Lx / (nx - 1);
dy = Ly / (ny - 1);
dt_max = (dx^2 * dy^2) / (2 * D * (dx^2 + dy^2));
dt = cfl_factor * dt_max;
nsteps = ceil(t_final / dt);
dt = t_final / nsteps; % land exactly on t_final after nsteps

fprintf('[MATLAB] grid=%dx%d, dx=%.5f, dy=%.5f\n', nx, ny, dx, dy);
fprintf('[MATLAB] dt=%.6f (dt_max=%.6f, safety factor=%.1f), nsteps=%d\n', dt, dt_max, cfl_factor, nsteps);

% --- Initial condition: Gaussian blob --------------------------------------
xs = (0:nx-1) * dx;
ys = (0:ny-1) * dy;
[XI, YJ] = ndgrid(xs, ys);  % ndgrid so XI(i,j)=xs(i), YJ(i,j)=ys(j), matching Qu's U[i,j]=f(i*dx,j*dy)

R2 = (XI - x0).^2 + (YJ - y0).^2;
U = A0 * exp(-R2 / (2 * sigma0^2));

mass0 = sum(U(:)) * dx * dy;

m1 = memory();
peak_mem_bytes = max(peak_mem_bytes, m1.MemUsedMATLAB);

% --- Time-stepping: explicit FTCS diffusion, vectorized slicing -----------
tic;
for step = 1:nsteps
    Uold = U;
    d2x = (Uold(3:end, 2:end-1) - 2*Uold(2:end-1, 2:end-1) + Uold(1:end-2, 2:end-1)) / dx^2;
    d2y = (Uold(2:end-1, 3:end) - 2*Uold(2:end-1, 2:end-1) + Uold(2:end-1, 1:end-2)) / dy^2;
    U(2:end-1, 2:end-1) = Uold(2:end-1, 2:end-1) + D * dt * (d2x + d2y);
    if mod(step, 20) == 0
        mstep = memory();
        peak_mem_bytes = max(peak_mem_bytes, mstep.MemUsedMATLAB);
    end
end
elapsed_ms = toc * 1000;

mfinal = memory();
peak_mem_bytes = max(peak_mem_bytes, mfinal.MemUsedMATLAB);

mass_final = sum(U(:)) * dx * dy;

% --- Verification: analytic Gaussian reference at t_final ------------------
sigma_final_sq = sigma0^2 + 2 * D * t_final;
amp_final = A0 * (sigma0^2 / sigma_final_sq);
Uexact = amp_final * exp(-R2 / (2 * sigma_final_sq));

diffU = U - Uexact;
max_abs_err = max(abs(diffU(:)));
rms_err = sqrt(mean(diffU(:).^2));
max_field_val = max(Uexact(:));

fprintf('[MATLAB] elapsed=%.1f ms for %d steps on a %dx%d grid\n', elapsed_ms, nsteps, nx, ny);
fprintf('[MATLAB] mass: initial=%.6f, final=%.6f, relative drift=%.4f%%\n', mass0, mass_final, (mass_final - mass0) / mass0 * 100);
fprintf('[MATLAB] vs analytic Gaussian at t=%.1f: max|err|=%.6e, rms(err)=%.6e, peak field value=%.6f\n', t_final, max_abs_err, rms_err, max_field_val);
fprintf('[MATLAB] max|err| as %% of peak field value: %.4f%%\n', max_abs_err / max_field_val * 100);

if max_abs_err / max_field_val < 0.02
    fprintf('[MATLAB] PASS: numeric field matches analytic Gaussian diffusion to <2%% of peak amplitude\n');
else
    fprintf('[MATLAB] FAIL: numeric field deviates from analytic Gaussian diffusion by >=2%% of peak amplitude\n');
end

% --- Cross-check against Qu's own final field, if it dumped one -----------
this_dir = fileparts(mfilename('fullpath'));
qu_csv = fullfile(this_dir, 'qu_fluid_field.csv');
if exist(qu_csv, 'file')
    U_qu = readmatrix(qu_csv);
    cross_diff = U - U_qu;
    fprintf('[MATLAB] vs Qu''s own final field (%s): max|diff|=%.6e, rms(diff)=%.6e\n', qu_csv, max(abs(cross_diff(:))), sqrt(mean(cross_diff(:).^2)));
end

fprintf('[MATLAB] peak MemUsedMATLAB (sampled) = %.1f MB\n', peak_mem_bytes / (1024*1024));
