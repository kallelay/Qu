% bench.m -- MATLAB port of matty's BenchmarkSuite, same kernels/sizes as bench.qu/bench.py.
%
% Run: matlab -batch "run('benchmarks/matty_suite/bench.m')"

tic;
A = randn(1000, 1000);
B = randn(1000, 1000);
C = A * B;
t_matmul = toc;
fprintf('matrix_multiply   : %.4f s\n', t_matmul);

tic;
A2 = randn(10000, 10000);
B2 = randn(10000, 10000);
C2 = A2 .* B2 + sin(A2) .* cos(B2);
D2 = abs(C2);
t_elemwise = toc;
fprintf('element_wise_ops  : %.4f s\n', t_elemwise);

tic;
x = randn(100000, 1);
X = fft(x);
xr = ifft(X);
t_fft = toc;
fprintf('fft               : %.4f s\n', t_fft);

tic;
Az = zeros(5000, 5000);
Bo = ones(5000, 5000);
Ce = eye(5000);
Dr = rand(5000, 5000);
t_arrcreate = toc;
fprintf('array_creation    : %.4f s\n', t_arrcreate);

tic;
result = 0;
for i = 1:100000
    result = result + i;
end
t_loop = toc;
fprintf('loop_performance  : %.4f s  (result = %d)\n', t_loop, result);

total = t_matmul + t_elemwise + t_fft + t_arrcreate + t_loop;
fprintf('total             : %.4f s\n', total);
