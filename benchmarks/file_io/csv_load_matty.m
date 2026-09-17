% csv_load_matty.m -- large multi-column CSV load benchmark, Matty side.
%
% Same shape as csv_load.qu/.py/.m (3,000,000 rows x 6 numeric columns),
% minus `rng(3)` (unsupported) and `table`/`readtable`/`writetable`
% (checked builtins.py: not present at all -- Matty only has
% `csvread`/`csvwrite` on plain matrices). Values won't be
% seed-reproducible, and there are no column names, but this exercises
% the same "load a several-million-row CSV" primitive.
%
% Run: python matty_runner.py csv_load_matty.m   (from the matty repo,
% with its own .venv active)

N = 3000000;
bin_path = 'matty_wide_data.csv';

M = [randn(N,1), randn(N,1), randn(N,1), randn(N,1), randn(N,1), randn(N,1)];
csvwrite(bin_path, M);

tic;
D = csvread(bin_path);
t_load = toc;
fprintf('csvread (%d rows x 6 cols) : %.4f s\n', N, t_load);

fprintf('mean(c1) = %.6f\n', mean(D(:,1)));
fprintf('mean(c6) = %.6f\n', mean(D(:,6)));
fprintf('nrow = %d  ncol = %d\n', size(D,1), size(D,2));
