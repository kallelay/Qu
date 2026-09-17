% bench_fft_factorization.m -- MATLAB companion to bench_fft_factorization.qu.
sizes = [1048576, 995328, 1000000, 1000003];
labels = {'2^20 (power of two)', '2^12*3^5 (3-smooth)', '2^6*5^6 (5-smooth)', '1000003 (prime)'};

for i = 1:4
    N = sizes(i);
    x = randn(1, N);
    X = fft(x); %#ok<NASGU> warm-up, discarded
    best_t = inf;
    for r = 1:3
        tic; X = fft(x); t = toc; %#ok<NASGU>
        best_t = min(best_t, t);
    end
    fprintf('%s: N=%d  time=%.6f s\n', labels{i}, N, best_t);
end
