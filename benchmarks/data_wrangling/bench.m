% bench.m -- Table/DataFrame operations benchmark, MATLAB side.
%
% Same dataset (dataset.csv, built once by generate_data.py) and same six
% steps as bench.qu/bench.py: load, filter, groupby+agg, sort, descriptive
% stats, correlation matrix -- using MATLAB's own `table`/`readtable`,
% logical indexing, `groupsummary`, `sortrows`, and `summary`/`corrcoef`.
%
% Run: matlab -batch "run('benchmarks/data_wrangling/bench.m')"

csv_path = 'benchmarks/data_wrangling/dataset.csv';
num_cols = {'x1', 'x2', 'x3', 'x4'};

% --- 1. load -----------------------------------------------------------------
tic;
T = readtable(csv_path);
t_load = toc;
fprintf('load          : %.4f s  (%d rows, %d cols)\n', t_load, height(T), width(T));

% --- 2. filter: x1 > 0 --------------------------------------------------------
tic;
filtered = T(T.x1 > 0, :);
t_filter = toc;
fprintf('filter        : %.4f s  (%d rows kept)\n', t_filter, height(filtered));

% --- 3. groupby(category) + agg(mean/sum/count of x2) -------------------------
% NOTE: `groupsummary`'s own 'nnz' method (nonzero count) is NOT the same
% as a row count -- it silently undercounts if any value in the group
% happens to equal exactly 0 (found the hard way: one x2 value in this
% dataset rounds to 0.000000 at 6 decimal places, so an earlier version of
% this script using 'nnz' reported 999,999 total rows instead of 1,000,000
% when cross-checked against pandas' `.agg(["count"])`). `groupsummary`
% always returns a `GroupCount` column (true per-group row count,
% independent of any requested aggregation methods) -- use that instead.
tic;
grouped = groupsummary(T, 'category', {'mean', 'sum'}, 'x2');
t_group = toc;
grouped = sortrows(grouped, 'category');
fprintf('groupby_agg   : %.4f s  (%d groups)\n', t_group, height(grouped));

% --- 4. sort by x3 -------------------------------------------------------------
tic;
sorted_T = sortrows(T, 'x3');
t_sort = toc;
fprintf('sort_by       : %.4f s\n', t_sort);

% --- 5. descriptive stats: count/mean/std/min/25/50/75/max on numeric cols ----
tic;
X = T{:, num_cols};
n = size(X, 1);
stat_count = repmat(n, 1, 4);
stat_mean = mean(X, 1);
stat_std = std(X, 0, 1);
stat_min = min(X, [], 1);
stat_p25 = prctile(X, 25, 1);
stat_p50 = prctile(X, 50, 1);
stat_p75 = prctile(X, 75, 1);
stat_max = max(X, [], 1);
t_describe = toc;
fprintf('describe      : %.4f s\n', t_describe);

% --- 6. correlation matrix ---------------------------------------------------
tic;
C = corrcoef(X);
t_corr = toc;
fprintf('corr_matrix   : %.4f s\n', t_corr);

total = t_load + t_filter + t_group + t_sort + t_describe + t_corr;
fprintf('total         : %.4f s\n', total);

% --- correctness cross-check printout ---------------------------------------
fprintf('\n=== correctness cross-check values ===\n');
fprintf('total_rows          = %d\n', height(T));
fprintf('filtered_rows(x1>0) = %d\n', height(filtered));
fprintf('group means (grp_0..grp_7, alphabetical): ');
fprintf('%.6f ', grouped.mean_x2);
fprintf('\n');
fprintf('group sums  (grp_0..grp_7, alphabetical): ');
fprintf('%.6f ', grouped.sum_x2);
fprintf('\n');
fprintf('group counts(grp_0..grp_7, alphabetical): ');
fprintf('%d ', grouped.GroupCount);
fprintf('\n');
fprintf('sorted_first_x3     = %.6f\n', sorted_T.x3(1));
fprintf('sorted_last_x3      = %.6f\n', sorted_T.x3(end));
fprintf('describe() stat order: count, mean, std, min, 25%%, 50%%, 75%%, max\n');
fprintf('describe().x1 = %.6f %.6f %.6f %.6f %.6f %.6f %.6f %.6f\n', stat_count(1), stat_mean(1), stat_std(1), stat_min(1), stat_p25(1), stat_p50(1), stat_p75(1), stat_max(1));
fprintf('describe().x2 = %.6f %.6f %.6f %.6f %.6f %.6f %.6f %.6f\n', stat_count(2), stat_mean(2), stat_std(2), stat_min(2), stat_p25(2), stat_p50(2), stat_p75(2), stat_max(2));
fprintf('describe().x3 = %.6f %.6f %.6f %.6f %.6f %.6f %.6f %.6f\n', stat_count(3), stat_mean(3), stat_std(3), stat_min(3), stat_p25(3), stat_p50(3), stat_p75(3), stat_max(3));
fprintf('describe().x4 = %.6f %.6f %.6f %.6f %.6f %.6f %.6f %.6f\n', stat_count(4), stat_mean(4), stat_std(4), stat_min(4), stat_p25(4), stat_p50(4), stat_p75(4), stat_max(4));
fprintf('corr matrix:\n');
disp(C);
