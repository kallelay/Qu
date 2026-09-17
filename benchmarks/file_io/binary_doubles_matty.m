% binary_doubles_matty.m -- packed float64 binary round trip, Matty side.
%
% Same as binary_doubles.m, minus `rng(42)` -- Matty has no `rng()`
% (checked: running binary_doubles.m unmodified against matty_runner.py
% fails immediately with "Undefined function or variable 'rng'"). Values
% won't be seed-reproducible against the Qu/Python/MATLAB runs, but
% fopen/fwrite/fread/fclose are all real, scipy/numpy-backed builtins in
% Matty and this scenario runs to completion and produces real,
% comparable timings.
%
% Run: python matty_runner.py binary_doubles_matty.m   (from the matty
% repo, with its own .venv active)

N = 1000000;
bin_path = 'matty_doubles.bin';

x = randn(N, 1);

tic;
fid = fopen(bin_path, 'wb');
fwrite(fid, x, 'double');
fclose(fid);
t_write = toc;
fprintf('fwrite (%d values) : %.4f s\n', N, t_write);

tic;
fid2 = fopen(bin_path, 'rb');
y = fread(fid2, N, 'double');
fclose(fid2);
t_read = toc;
fprintf('fread  (%d values) : %.4f s\n', N, t_read);

fprintf('mean(written) = %.6f\n', mean(x));
fprintf('mean(read)    = %.6f\n', mean(y));
fprintf('sumsq(read)   = %.6f\n', sum(y .* y));
