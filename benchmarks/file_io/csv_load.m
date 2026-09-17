% csv_load.m -- large multi-column CSV load benchmark, MATLAB side.
%
% Same shape as csv_load.qu/.py (3,000,000 rows x 6 numeric columns).
% Uses readtable, per the task's ask for MATLAB's own idiomatic table
% loader. Matty has no readtable (checked builtins.py: only csvread/
% csvwrite) -- see csv_load_matty.m for the csvread variant run against
% Matty instead, and the README for that incompatibility noted explicitly.
%
% Run (cd into this script's own directory first, same reasoning as
% binary_doubles.m):
%   matlab -batch "cd('benchmarks/file_io'); csv_load"

N = 3000000;
bin_path = 'matlab_wide_data.csv';

rng(3);
T = table(randn(N,1), randn(N,1), randn(N,1), randn(N,1), randn(N,1), randn(N,1), ...
    'VariableNames', {'c1','c2','c3','c4','c5','c6'});
writetable(T, bin_path);

tic;
df = readtable(bin_path);
t_load = toc;
fprintf('readtable (%d rows x 6 cols) : %.4f s\n', N, t_load);

fprintf('mean(c1) = %.6f\n', mean(df.c1));
fprintf('mean(c6) = %.6f\n', mean(df.c6));
fprintf('nrow = %d  ncol = %d\n', height(df), width(df));
