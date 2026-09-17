% binary_doubles.m -- packed float64 binary file round trip, MATLAB side.
%
% Vectorized fwrite/fread (one call each, not a per-value loop) -- the
% natural MATLAB idiom. Same script is also run, unmodified, through
% Matty's own matty_runner.py (both fopen/fwrite/fread/fclose exist in
% matty's builtins.py) -- see the README for whether that succeeds.
%
% Run (cd into this script's own directory first -- MATLAB's `run()` does
% this internally too, which is why paths here are bare filenames, not
% "benchmarks/file_io/..." -- a relative path written against the repo
% root breaks under `run()`, which cd's into the script's own folder
% before executing it):
%   matlab -batch "cd('benchmarks/file_io'); binary_doubles"

N = 1000000;
bin_path = 'matlab_doubles.bin';

rng(42);
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
