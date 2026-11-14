//! Filter-design builtins added in v0.4.13, through the language:
//! `buttord`/`cheb1ord`/`cheb2ord`/`ellipord`, `kaiserord`, `remez`/`firpm`
//! and `window`. The numerics are unit-tested in `qu_core::filter_design`
//! (against SciPy and the textbook formulas); these check the builtins: the
//! arguments reach the right place, the records feed the design functions,
//! and the designs meet what was asked for, measured with `freqz`.

use qu_interp::{Interp, Value};

fn run(src: &str) -> Interp {
    let mut it = Interp::new();
    it.run(src).unwrap_or_else(|e| panic!("run failed: {e}\nsrc:\n{src}"));
    it
}

fn err(src: &str) -> String {
    let mut it = Interp::new();
    it.run(src).expect_err("expected an error").to_string()
}

fn nums(it: &Interp, n: &str) -> Vec<f64> {
    match it.get(n) {
        Some(Value::Vec(v)) => v.to_vec(),
        Some(Value::Num(x)) => vec![*x],
        other => panic!("`{n}` is {other:?}"),
    }
}

fn num(it: &Interp, n: &str) -> f64 {
    nums(it, n)[0]
}

fn text(it: &Interp, n: &str) -> String {
    match it.get(n) {
        Some(Value::Str(s)) => s.to_string(),
        other => panic!("`{n}` is {other:?}"),
    }
}

fn close(a: &[f64], b: &[f64], tol: f64) {
    assert_eq!(a.len(), b.len(), "{a:?} vs {b:?}");
    for (x, y) in a.iter().zip(b) {
        assert!((x - y).abs() <= tol, "{a:?} vs {b:?}");
    }
}

/// Worst passband loss and smallest stopband attenuation (positive dB) of a
/// magnitude response `mag` sampled from 0 to `fs/2`.
fn measure(mag: &[f64], fs: f64, pass: &dyn Fn(f64) -> bool, stop: &dyn Fn(f64) -> bool) -> (f64, f64) {
    let n = mag.len();
    let (mut worst_pass, mut worst_stop) = (0.0f64, f64::INFINITY);
    for (k, m) in mag.iter().enumerate() {
        let f = fs / 2.0 * k as f64 / (n - 1) as f64;
        let db = -20.0 * m.max(1e-300).log10();
        if pass(f) {
            worst_pass = worst_pass.max(db);
        }
        if stop(f) {
            worst_stop = worst_stop.min(db);
        }
    }
    (worst_pass, worst_stop)
}

// --------------------------------------------------------- order estimators

#[test]
fn a_filter_designed_from_an_order_estimate_meets_the_specification() {
    let (fs, rp, rs) = (1000.0, 1.0, 40.0);
    let families = [
        ("buttord", "butter(r.order, r.kind, r.wn, fs)"),
        ("cheb1ord", "cheby1(r.order, rp, r.kind, r.wn, fs)"),
        ("cheb2ord", "cheby2(r.order, rs, r.kind, r.wn, fs)"),
        ("ellipord", "ellip(r.order, rp, rs, r.kind, r.wn, fs)"),
    ];
    // (wp, ws, kind, passband, stopband)
    type Band = Box<dyn Fn(f64) -> bool>;
    let specs: Vec<(&str, &str, &str, Band, Band)> = vec![
        ("100", "150", "low", Box::new(|f| f <= 100.0), Box::new(|f| f >= 150.0)),
        ("150", "100", "high", Box::new(|f| f >= 150.0), Box::new(|f| f <= 100.0)),
        ("[100, 250]", "[50, 300]", "band", Box::new(|f| (100.0..=250.0).contains(&f)), Box::new(|f| f <= 50.0 || f >= 300.0)),
        ("[50, 300]", "[100, 250]", "stop", Box::new(|f| f <= 50.0 || f >= 300.0), Box::new(|f| (100.0..=250.0).contains(&f))),
    ];
    for (ord, design) in families {
        for (wp, ws, kind, pass, stop) in &specs {
            let it = run(&format!(
                "fs = {fs}\nrp = {rp}\nrs = {rs}\nr = {ord}({wp}, {ws}, rp, rs, fs)\nkind = r.kind\norder = r.order\nfilt = {design}\nmag = abs(freqz(filt, 8001))\nok = is_stable(filt)"
            ));
            assert_eq!(text(&it, "kind"), *kind, "{ord} {wp} {ws}");
            assert!(matches!(it.get("ok"), Some(Value::Bool(true))), "{ord} {kind}: unstable");
            let (worst_pass, worst_stop) = measure(&nums(&it, "mag"), fs, pass.as_ref(), stop.as_ref());
            let order = num(&it, "order");
            assert!(worst_pass <= rp + 1e-6, "{ord} {kind} (order {order}): passband loses {worst_pass} dB, allowed {rp}");
            assert!(worst_stop >= rs - 1e-6, "{ord} {kind} (order {order}): stopband only {worst_stop} dB down, wanted {rs}");
        }
    }
}

#[test]
fn order_estimates_match_scipy_through_the_language() {
    // scipy.signal.buttord(0.2, 0.3, 1, 40) -> (12, 0.21077527313622169);
    // ellipord -> (4, 0.2); cheb2ord([0.2, 0.5], [0.1, 0.6], 1, 40) ->
    // (6, [0.15706857725752188, 0.580219828322454]). fs = 2 is normalized.
    let it = run("a = buttord(0.2, 0.3, 1, 40, 2)\nao = a.order\naw = a.wn\nb = ellipord(0.2, 0.3, 1, 40, fs=2)\nbo = b.order\nbw = b.wn\nc = cheb2ord([0.2, 0.5], [0.1, 0.6], 1, 40, 2)\nco = c.order\ncw = c.wn\nck = c.kind");
    assert_eq!(num(&it, "ao"), 12.0);
    close(&nums(&it, "aw"), &[0.21077527313622169], 1e-12);
    assert_eq!(num(&it, "bo"), 4.0);
    close(&nums(&it, "bw"), &[0.2], 1e-12);
    assert_eq!(num(&it, "co"), 6.0);
    close(&nums(&it, "cw"), &[0.15706857725752188, 0.580219828322454], 1e-12);
    assert_eq!(text(&it, "ck"), "band");
    // Hz and normalized agree.
    let it = run("a = buttord(100, 150, 1, 40, 1000)\nw = a.wn\nn = a.order");
    assert_eq!(num(&it, "n"), 12.0);
    close(&nums(&it, "w"), &[0.21077527313622169 * 500.0], 1e-9);
}

#[test]
fn order_estimates_explain_a_bad_specification() {
    assert!(err("r = buttord(100, 150, 1, 40)").contains("needs 5 arguments"));
    let msg = err("r = buttord(100, 600, 1, 40, 1000)");
    assert!(msg.contains("buttord") && msg.contains("fs/2"), "{msg}");
    let msg = err("r = cheb1ord([100, 200], [150, 300], 1, 40, 1000)");
    assert!(msg.contains("bandpass") && msg.contains("bandstop"), "{msg}");
    let msg = err("r = ellipord(100, 150, 40, 1, 1000)");
    assert!(msg.contains("rs"), "{msg}");
}

// ---------------------------------------------------------------- kaiserord

#[test]
fn a_kaiser_fir_designed_from_kaiserord_meets_the_ripple_specification() {
    // Pass to 1 kHz, stop from 1.5 kHz at 8 kHz; 1 % ripple, 60 dB.
    let it = run("fs = 8000\nr = kaiserord([1000, 1500], [1, 0], [0.01, 0.001], fs)\norder = r.order\nbeta = r.beta\nwn = r.wn\nkind = r.kind\nfilt = fir1(r.order, r.wn, kind=r.kind, window=\"kaiser\", beta=r.beta, fs=fs)\nmag = abs(freqz(filt, 8001))");
    // A = 60 dB: beta = 0.1102 (60 - 8.7); order = ceil((60 - 7.95) / (2.285 * 2 pi * 500 / 8000)).
    close(&nums(&it, "beta"), &[0.1102 * 51.3], 1e-12);
    let expected = ((60.0 - 7.95) / (2.285 * 2.0 * std::f64::consts::PI * 500.0 / 8000.0)).ceil();
    assert_eq!(num(&it, "order"), expected);
    assert_eq!(expected, 59.0);
    assert_eq!(num(&it, "wn"), 1250.0);
    assert_eq!(text(&it, "kind"), "low");
    let mag = nums(&it, "mag");
    let n = mag.len();
    let (mut dp, mut ds) = (0.0f64, 0.0f64);
    for (k, m) in mag.iter().enumerate() {
        let f = 4000.0 * k as f64 / (n - 1) as f64;
        if f <= 1000.0 {
            dp = dp.max((m - 1.0).abs());
        }
        if f >= 1500.0 {
            ds = ds.max(*m);
        }
    }
    assert!(dp <= 0.01, "passband deviation {dp}");
    assert!(ds <= 0.001 * 1.02, "stopband deviation {ds}");
    // normalized frequencies by default, fs= by name
    let it = run("a = kaiserord([0.25, 0.375], [1, 0], 0.001)\nb = kaiserord([1000, 1500], [1, 0], 0.001, fs=8000)\nao = a.order\nbo = b.order\naw = a.wn");
    assert_eq!(num(&it, "ao"), num(&it, "bo"));
    close(&nums(&it, "aw"), &[0.3125], 1e-12);
    let msg = err("r = kaiserord([0.2, 0.3], [1, 1], 0.01)");
    assert!(msg.contains("kaiserord") && msg.contains("alternate"), "{msg}");
}

// -------------------------------------------------------------------- remez

#[test]
fn remez_designs_the_scipy_reference_filter() {
    // scipy.signal.remez(21, [0, 0.3, 0.4, 1], [1, 0], fs=2, grid_density=4096)
    let reference = [
        -0.03796584539600822, -0.0084514392873488, 0.02065528411215688, 0.03928173761604945,
        0.0167372292746837, -0.03805178424910088, -0.06841126830904648, -0.01362634272418903,
        0.12901107625335648, 0.2848114706977272, 0.3523612852077771,
    ];
    let it = run("filt = remez(20, [0, 0.3, 0.4, 1], [1, 1, 0, 0])\nb = impz(filt)\nalias = impz(firpm(20, [0, 0.3, 0.4, 1], [1, 1, 0, 0]))\nhz = impz(remez(20, [0, 1500, 2000, 5000], [1, 1, 0, 0], fs=10000))\nhz2 = impz(remez(20, [0, 1500, 2000, 5000], [1, 1, 0, 0], [1, 1], 10000))\ninfo = filter_info(filt)\ndelay = info.delay_samples\ndc = info.dc_gain\nstable = info.stable\ngd = group_delay(filt, 64)");
    let b = nums(&it, "b");
    assert_eq!(b.len(), 21);
    close(&b[..11], &reference, 1e-6);
    close(&b, &nums(&it, "alias"), 0.0);
    close(&b, &nums(&it, "hz"), 1e-13);
    close(&b, &nums(&it, "hz2"), 1e-13);
    // linear phase: the delay is half the order, everywhere
    assert!((num(&it, "delay") - 10.0).abs() < 1e-9);
    for g in nums(&it, "gd") {
        assert!((g - 10.0).abs() < 1e-6, "group delay {g}");
    }
    // DC is inside the passband ripple of 0.0555130
    assert!((num(&it, "dc") - 1.0).abs() <= 0.0555131, "{}", num(&it, "dc"));
    assert!(matches!(it.get("stable"), Some(Value::Bool(true))));
}

#[test]
fn remez_is_equiripple_and_beats_a_hamming_fir1_of_the_same_order() {
    let it = run("eq = abs(freqz(remez(40, [0, 0.3, 0.4, 1], [1, 1, 0, 0], weights=[1, 10]), 16001))\nwin = abs(freqz(fir1(40, 0.35), 16001))");
    let (eq, win) = (nums(&it, "eq"), nums(&it, "win"));
    let n = eq.len();
    let f = |k: usize| k as f64 / (n - 1) as f64;
    // stopband peaks of the equiripple design: all the same height
    let mut peaks = Vec::new();
    for k in 1..n - 1 {
        if f(k) >= 0.4 && eq[k] >= eq[k - 1] && eq[k] > eq[k + 1] {
            peaks.push(eq[k]);
        }
    }
    let top = peaks.iter().cloned().fold(0.0, f64::max);
    assert!(peaks.len() >= 10, "{} stopband ripples", peaks.len());
    for p in &peaks {
        assert!((p - top).abs() <= 1e-3 * top, "ripple {p} vs {top}");
    }
    let stop = |m: &[f64]| (0..n).filter(|&k| f(k) >= 0.4).map(|k| m[k]).fold(0.0, f64::max);
    let (db_eq, db_win) = (-20.0 * stop(&eq).log10(), -20.0 * stop(&win).log10());
    assert!(db_eq > db_win + 10.0, "remez {db_eq} dB, Hamming fir1 {db_win} dB");
}

#[test]
fn remez_says_what_it_does_not_build() {
    let msg = err("f = remez(21, [0, 0.3, 0.4, 1], [0, 0, 1, 1])");
    assert!(msg.contains("remez") && msg.contains("Nyquist") && msg.contains("order 22"), "{msg}");
    let msg = err("f = firpm(20, [0, 0.3, 0.4, 1], [1, 1, 0, 0], type=\"hilbert\")");
    assert!(msg.contains("firpm") && msg.contains("antisymmetric"), "{msg}");
    let msg = err("f = remez(20, [0, 0.3, 0.4, 1], [1, 0])");
    assert!(msg.contains("one amplitude per band edge"), "{msg}");
    let msg = err("f = remez(20, [0, 0.3, 0.4, 1], [1, 1, 0, 0], colour=3)");
    assert!(msg.contains("unknown keyword"), "{msg}");
    // odd order lowpass (Type II) is fine
    let it = run("b = impz(remez(21, [0, 0.3, 0.4, 1], [1, 1, 0, 0], [1, 10]))");
    let b = nums(&it, "b");
    assert_eq!(b.len(), 22);
    // scipy.signal.remez(22, [0, .3, .4, 1], [1, 0], weight=[1, 10], fs=2, grid_density=4096)
    close(&b[..3], &[-0.00791203496986663, 0.01388934381237734, 0.03970498016748691], 1e-6);
}

// ------------------------------------------------------------------- window

#[test]
fn window_returns_the_known_values() {
    let it = run(concat!(
        "h5 = window(\"hann\", 5)\n",
        "hp = window(\"hann\", 4, sym=false)\n",
        "hp2 = window(\"hann\", 4, false)\n",
        "ham = window(\"hamming\", 5)\n",
        "bl = window(\"blackman\", 5)\n",
        "re = window(\"rect\", 3)\n",
        "ba = window(\"bartlett\", 5)\n",
        "tu = window(\"tukey\", 9)\n",
        "tu1 = window(\"tukey\", 5, alpha=1)\n",
        "ga = window(\"gaussian\", 5, std=1)\n",
        "bh = window(\"blackmanharris\", 9)\n",
        "nu = window(\"nuttall\", 9)\n",
        "ft = window(\"flattop\", 9)\n",
        "ftp = window(\"flattop\", 8, sym=false)\n",
        "ka = window(\"kaiser\", 9, beta=5)\n",
        "kb = kaiser(9, 5)\n",
        "one = window(\"hann\", 1)\n",
        "empty_len = length(window(\"hann\", 0))\n",
    ));
    close(&nums(&it, "h5"), &[0.0, 0.5, 1.0, 0.5, 0.0], 1e-15);
    close(&nums(&it, "hp"), &[0.0, 0.5, 1.0, 0.5], 1e-15);
    close(&nums(&it, "hp2"), &[0.0, 0.5, 1.0, 0.5], 1e-15);
    close(&nums(&it, "ham"), &[0.08, 0.54, 1.0, 0.54, 0.08], 1e-15);
    close(&nums(&it, "bl"), &[0.0, 0.34, 1.0, 0.34, 0.0], 1e-15);
    close(&nums(&it, "re"), &[1.0, 1.0, 1.0], 0.0);
    close(&nums(&it, "ba"), &[0.0, 0.5, 1.0, 0.5, 0.0], 1e-15);
    close(&nums(&it, "tu"), &[0.0, 0.5, 1.0, 1.0, 1.0, 1.0, 1.0, 0.5, 0.0], 1e-15);
    close(&nums(&it, "tu1"), &[0.0, 0.5, 1.0, 0.5, 0.0], 1e-15);
    let (e2, eh) = ((-2.0f64).exp(), (-0.5f64).exp());
    close(&nums(&it, "ga"), &[e2, eh, 1.0, eh, e2], 1e-15);
    // scipy.signal.get_window(name, 9, fftbins=False)
    close(
        &nums(&it, "bh"),
        &[6.0e-05, 2.1735837018679628e-02, 2.1747e-01, 6.9576416298132038e-01, 1.0, 6.9576416298132038e-01, 2.1747e-01, 2.1735837018679628e-02, 6.0e-05],
        1e-14,
    );
    close(
        &nums(&it, "nu"),
        &[3.628e-04, 2.5205566515401824e-02, 2.269824e-01, 7.0195823348459818e-01, 1.0, 7.0195823348459818e-01, 2.269824e-01, 2.5205566515401824e-02, 3.628e-04],
        1e-14,
    );
    let flattop = [-4.21051e-04, -2.6872193286334545e-02, -5.473684e-02, 4.4413535728633452e-01, 1.000000003, 4.4413535728633452e-01, -5.473684e-02, -2.6872193286334545e-02, -4.21051e-04];
    close(&nums(&it, "ft"), &flattop, 1e-14);
    // scipy.signal.get_window("flattop", 8, fftbins=True): the same, less the last sample
    close(&nums(&it, "ftp"), &flattop[..8], 1e-14);
    // scipy.signal.get_window(("kaiser", 5.0), 9, fftbins=False)
    close(
        &nums(&it, "ka")[..5],
        &[0.03671089227128668, 0.23054433409868885, 0.5528517696991324, 0.868017159478478, 1.0],
        1e-12,
    );
    close(&nums(&it, "ka"), &nums(&it, "kb"), 0.0);
    close(&nums(&it, "one"), &[1.0], 0.0);
    assert_eq!(num(&it, "empty_len"), 0.0);
}

#[test]
fn window_matches_the_generator_functions_bit_for_bit() {
    let it = run("a = window(\"hann\", 33)\nb = hann(33)\nc = window(\"hamming\", 33)\nd = hamming(33)\ne = window(\"blackman\", 33)\ng = blackman(33)");
    close(&nums(&it, "a"), &nums(&it, "b"), 0.0);
    close(&nums(&it, "c"), &nums(&it, "d"), 0.0);
    close(&nums(&it, "e"), &nums(&it, "g"), 0.0);
}

#[test]
fn window_reports_a_wrong_name_or_parameter() {
    let msg = err("w = window(\"hanx\", 8)");
    assert!(msg.contains("unknown window `hanx`") && msg.contains("flattop"), "{msg}");
    assert!(err("w = window(\"kaiser\", 8)").contains("beta="));
    assert!(err("w = window(\"gaussian\", 8)").contains("std="));
    assert!(err("w = window(\"tukey\", 8, alpha=2)").contains("between 0 and 1"));
    // a parameter the window does not have is not silently dropped
    let msg = err("w = window(\"hann\", 8, beta=3)");
    assert!(msg.contains("unknown keyword"), "{msg}");
}

#[test]
fn fir1_and_welch_take_every_window_name() {
    // fir1: taps are the ideal response times the named window, rescaled to
    // unit DC gain; so the ratio of two designs is the ratio of the windows.
    let it = run(concat!(
        "d = impz(fir1(20, 0.3))\n",
        "h = impz(fir1(20, 0.3, window=\"hamming\"))\n",
        "b = impz(fir1(20, 0.3, window=\"blackmanharris\"))\n",
        "t = impz(fir1(20, 0.3, window=\"tukey\", alpha=0.25))\n",
        "r = impz(fir1(20, 0.3, window=\"rect\"))\n",
        "wb = window(\"blackmanharris\", 21)\n",
        "wt = window(\"tukey\", 21, alpha=0.25)\n",
    ));
    close(&nums(&it, "d"), &nums(&it, "h"), 0.0);
    let (r, b, t) = (nums(&it, "r"), nums(&it, "b"), nums(&it, "t"));
    let (wb, wt) = (nums(&it, "wb"), nums(&it, "wt"));
    for (taps, win) in [(&b, &wb), (&t, &wt)] {
        // un-normalize both through the centre tap (window value 1 there)
        for k in 0..21 {
            let expected = r[k] / r[10] * win[k];
            assert!((taps[k] / taps[10] - expected).abs() < 1e-12, "tap {k}");
        }
        assert!((taps.iter().sum::<f64>() - 1.0).abs() < 1e-12, "unit DC gain");
    }
    assert!(err("f = fir1(20, 0.3, window=\"nope\")").contains("fir1: unknown window `nope`"));
    assert!(err("f = fir1(20, 0.3, window=\"kaiser\")").contains("fir1: window=\"kaiser\" needs a beta= argument"));

    // welch: a constant-amplitude tone keeps its total power under any
    // window (density scaling divides by the window's own power).
    let it = run(concat!(
        "fs = 1024\n",
        "t = linspace(0, 1 - 1/fs, 1024)\n",
        "x = sin(2 * pi * 100 * t)\n",
        "ph = sum(welch(x, fs, nperseg=256)) * fs / 256\n",
        "pb = sum(welch(x, fs, nperseg=256, window=\"blackmanharris\")) * fs / 256\n",
        "pt = sum(welch(x, fs, nperseg=256, window=\"tukey\", alpha=0.5)) * fs / 256\n",
        "pf = sum(welch(x, fs, nperseg=256, window=\"flattop\")) * fs / 256\n",
        "pg = sum(welch(x, fs, nperseg=256, window=\"gaussian\", std=40)) * fs / 256\n",
    ));
    for name in ["ph", "pb", "pt", "pf", "pg"] {
        // mean square of a unit sine is 1/2
        assert!((num(&it, name) - 0.5).abs() < 5e-3, "{name} = {}", num(&it, name));
    }
    assert!(err("p = welch(sin(linspace(0, 50, 512)), 100, window=\"nope\")").contains("welch: unknown window `nope`"));
}
