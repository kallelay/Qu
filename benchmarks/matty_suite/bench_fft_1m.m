% bench_fft_1m.m -- MATLAB port of bench_fft_1m.qu.
%
% Same exact-bin-sinusoid ground truth and phase convention, same
% methodology. fft/ifft are base MATLAB (confirmed already working in
% bench_fair.m earlier this session) -- unlike bench_filter_accuracy.m,
% this needs no toolbox.

run_case(1048576, 'power-of-two  ');
run_case(1000000, 'non-power-of-2');

function run_case(N, label)
    sig = make_signal(N);

    X = fft(sig.x); %#ok<NASGU> warm-up, discarded
    best_t = inf;
    xr = [];
    for r = 1:3
        tic;
        X = fft(sig.x);
        xr = ifft(X);
        t = toc;
        best_t = min(best_t, t);
    end

    [max_signal_err, max_noise] = check_accuracy(X, sig, N);
    roundtrip_err = max(abs(xr - sig.x));

    fprintf('%s (N=%d): time=%.6f s   max signal-bin error=%.2e   noise floor=%.2e   roundtrip |ifft(fft(x))-x|=%.2e\n', ...
        label, N, best_t, max_signal_err, max_noise, roundtrip_err);
end

function sig = make_signal(N)
    k1 = 137; A1 = 1.0; phi1 = 0.0;
    k2 = 9973; A2 = 2.5; phi2 = pi / 3.0;
    k3 = round(N / 5.0) + 11; A3 = 0.7; phi3 = -pi / 4.0;

    n = 0:(N-1);
    x = A1 * cos(2.0 * pi * k1 * n / N) + A2 * cos(2.0 * pi * k2 * n / N + phi2) + A3 * cos(2.0 * pi * k3 * n / N + phi3);

    sig.x = x; sig.k1 = k1; sig.A1 = A1; sig.phi1 = phi1;
    sig.k2 = k2; sig.A2 = A2; sig.phi2 = phi2;
    sig.k3 = k3; sig.A3 = A3; sig.phi3 = phi3;
end

function [max_signal_err, max_noise] = check_accuracy(X, sig, N)
    % MATLAB fft() is 1-indexed: bin k (0-based) is X(k+1).
    bins0 = [sig.k1, N - sig.k1, sig.k2, N - sig.k2, sig.k3, N - sig.k3];
    amps = [sig.A1, sig.A1, sig.A2, sig.A2, sig.A3, sig.A3];
    phis = [sig.phi1, -sig.phi1, sig.phi2, -sig.phi2, sig.phi3, -sig.phi3];

    max_signal_err = 0.0;
    for i = 1:6
        k0 = bins0(i);
        expected = (N * amps(i) / 2.0) * (cos(phis(i)) + 1i * sin(phis(i)));
        err = abs(X(k0 + 1) - expected);
        max_signal_err = max(max_signal_err, err);
    end

    max_noise = 0.0;
    for k0 = 0:997:(N-1)
        if ~any(bins0 == k0)
            m = abs(X(k0 + 1));
            max_noise = max(max_noise, m);
        end
    end
end
