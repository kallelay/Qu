//! SINAD, SNR, THD, SFDR and ENOB from a sampled record.
//!
//! All of these are the same measurement read four ways: split a signal's
//! power between what you meant to put there and everything else, then
//! report the ratio. What differs is where the line is drawn.
//!
//!   * **SINAD** -- signal against noise *and* distortion. Everything that
//!     is not excitation counts against you, harmonics included. This is
//!     the honest number for a converter, and the one ENOB is defined
//!     from.
//!   * **SNR** -- signal against noise only, with the harmonics excluded
//!     from both sides. Flatters a distorting system, which is why a
//!     datasheet quoting SNR without SINAD is telling you something.
//!   * **THD** -- the harmonics alone, against the fundamental.
//!   * **SFDR** -- the fundamental against the single worst spur, wherever
//!     it is. What matters when one tone must be visible next to another.
//!
//! Why this exists here: the crest-factor work is about how much amplitude
//! headroom a multisine wastes, and headroom is only interesting because
//! it converts into converter resolution. `ENOB = (SINAD - 1.76) / 6.02`
//! is the bridge, and without SINAD the whole argument has to be taken on
//! trust rather than measured from a record.
//!
//! **Multisine, not just single tone.** The textbook definition assumes
//! one fundamental. A multisine has tens of them, and picking "the largest
//! bin" would count every other excitation tone as noise -- giving a
//! SINAD tens of dB too low. So the excitation set is a parameter, and the
//! single-tone case is just the one-element instance of it.

use crate::{rfft_real, Complex64, NumericError};

/// How a record's power divided up.
#[derive(Debug, Clone, PartialEq)]
pub struct Sinad {
    /// Signal-to-noise-and-distortion ratio, dB.
    pub sinad_db: f64,
    /// Signal-to-noise ratio with harmonics excluded, dB. `None` when no
    /// harmonics fall inside the band, where it would equal `sinad_db`
    /// and reporting it separately would imply a distinction that was not
    /// measured.
    pub snr_db: Option<f64>,
    /// Total harmonic distortion, dB relative to the fundamental (so a
    /// more negative number is better). `None` for a multisine, where
    /// "the" harmonic series is not defined.
    pub thd_db: Option<f64>,
    /// Spurious-free dynamic range, dB: the weakest excitation tone above
    /// the strongest non-excitation bin.
    pub sfdr_db: f64,
    /// Effective number of bits, from SINAD by the ideal-converter
    /// relation.
    pub enob: f64,
    /// Bin indices treated as excitation.
    pub signal_bins: Vec<usize>,
    /// Power in those bins, and in everything else (DC excluded).
    pub signal_power: f64,
    pub noise_power: f64,
}

/// `ENOB = (SINAD - 1.76 dB) / 6.02 dB`.
///
/// The constants are the ideal uniform quantizer's: a full-scale sine
/// through an N-bit converter has SINAD = 6.02 N + 1.76 dB, so inverting
/// it states a measured SINAD as the resolution that would have produced
/// it. Anything below about 1.76 dB gives a negative ENOB, which is
/// meaningful -- the noise exceeds the signal -- and is returned rather
/// than clamped.
pub fn enob_from_sinad(sinad_db: f64) -> f64 {
    (sinad_db - 1.76) / 6.02
}

/// Power spectrum of a real record, one-sided, DC included at index 0.
///
/// No window is applied. That is deliberate and it is a constraint on the
/// caller, not an oversight: these measurements assume every tone lands
/// exactly on a bin (a coherently sampled record, which is how multisine
/// excitation is generated in the first place). A window would spread each
/// tone across neighbours and move its energy into what this counts as
/// noise. `leak` below is what handles the residual.
fn power_spectrum(x: &[f64]) -> Result<Vec<f64>, NumericError> {
    let spec: Vec<Complex64> = rfft_real(x)?;
    let n = x.len() as f64;
    Ok(spec.iter().map(|c| (c.re * c.re + c.im * c.im) / (n * n)).collect())
}

/// Sum the power in a bin and `leak` neighbours either side.
fn band_power(psd: &[f64], center: usize, leak: usize) -> f64 {
    let lo = center.saturating_sub(leak);
    let hi = (center + leak + 1).min(psd.len());
    psd[lo..hi].iter().sum()
}

/// Mark a bin and its neighbours as accounted for.
fn mark(taken: &mut [bool], center: usize, leak: usize) {
    let lo = center.saturating_sub(leak);
    let hi = (center + leak + 1).min(taken.len());
    for t in &mut taken[lo..hi] {
        *t = true;
    }
}

/// SINAD and friends for a record whose excitation is at the given bins.
///
/// `signal_bins` are one-sided FFT bin indices. Empty means "find it":
/// the largest non-DC bin is taken as the fundamental, which is the
/// single-tone case.
///
/// `leak` is how many bins either side of each tone are counted as part
/// of it. Zero is correct for a perfectly coherent record; 1 or 2 absorbs
/// the smear from a slightly incoherent one. Setting it too high hides
/// close-in noise inside the signal and flatters the result, so it stays
/// the caller's explicit choice rather than a hidden default.
///
/// DC is always excluded from both signal and noise: an offset is a
/// separate defect and counting it as noise would swamp everything else
/// in a record that merely sits off zero.
pub fn sinad_at_bins(
    x: &[f64],
    signal_bins: &[usize],
    leak: usize,
) -> Result<Sinad, NumericError> {
    if x.len() < 4 {
        return Err(NumericError::Decomposition(
            "sinad: need at least 4 samples".into(),
        ));
    }
    let psd = power_spectrum(x)?;
    if psd.len() < 3 {
        return Err(NumericError::Decomposition(
            "sinad: record is too short to have a spectrum".into(),
        ));
    }

    // Which bins carry excitation.
    let bins: Vec<usize> = if signal_bins.is_empty() {
        let (best, _) = psd
            .iter()
            .enumerate()
            .skip(1)
            .fold((1usize, f64::NEG_INFINITY), |(bi, bv), (i, &v)| {
                if v > bv {
                    (i, v)
                } else {
                    (bi, bv)
                }
            });
        vec![best]
    } else {
        let mut b: Vec<usize> = signal_bins.iter().copied().filter(|&i| i < psd.len() && i > 0).collect();
        b.sort_unstable();
        b.dedup();
        if b.is_empty() {
            return Err(NumericError::Decomposition(
                "sinad: none of the given bins fall inside the spectrum (bin 0 is DC and is excluded)".into(),
            ));
        }
        b
    };

    let mut taken = vec![false; psd.len()];
    taken[0] = true; // DC
    let mut signal_power = 0.0;
    for &b in &bins {
        signal_power += band_power(&psd, b, leak);
        mark(&mut taken, b, leak);
    }
    let noise_power: f64 = psd
        .iter()
        .enumerate()
        .filter(|(i, _)| !taken[*i])
        .map(|(_, &v)| v)
        .sum();

    if signal_power <= 0.0 {
        return Err(NumericError::Decomposition(
            "sinad: no power in the excitation bins".into(),
        ));
    }
    // A noise floor of exactly zero happens with synthetic input (an
    // ideal sine at an exact bin) and means infinite SINAD. Reporting
    // `inf` is more honest than a made-up ceiling, and the caller can
    // test for it.
    let sinad_db = if noise_power > 0.0 {
        10.0 * (signal_power / noise_power).log10()
    } else {
        f64::INFINITY
    };

    // Harmonics: only defined for a single fundamental. For a multisine
    // every harmonic of every tone lands on some other tone's bin as
    // often as not, and "the" harmonic series has no meaning.
    let (snr_db, thd_db) = if bins.len() == 1 {
        let f0 = bins[0];
        let mut harmonic_power = 0.0;
        let mut h = 2usize;
        while f0 * h < psd.len() {
            harmonic_power += band_power(&psd, f0 * h, leak);
            h += 1;
        }
        let noise_only = (noise_power - harmonic_power).max(0.0);
        let snr = if noise_only > 0.0 {
            Some(10.0 * (signal_power / noise_only).log10())
        } else {
            None
        };
        let thd = if harmonic_power > 0.0 {
            Some(10.0 * (harmonic_power / signal_power).log10())
        } else {
            None
        };
        (snr, thd)
    } else {
        (None, None)
    };

    // SFDR: the weakest tone the caller cares about, over the strongest
    // thing they do not. Using the weakest rather than the strongest tone
    // is the multisine reading of it -- a spur that buries the smallest
    // excitation has destroyed the measurement even if the largest tone
    // still towers over it.
    let weakest_tone = bins
        .iter()
        .map(|&b| band_power(&psd, b, leak))
        .fold(f64::INFINITY, f64::min);
    let worst_spur = psd
        .iter()
        .enumerate()
        .filter(|(i, _)| !taken[*i])
        .map(|(_, &v)| v)
        .fold(0.0f64, f64::max);
    let sfdr_db = if worst_spur > 0.0 {
        10.0 * (weakest_tone / worst_spur).log10()
    } else {
        f64::INFINITY
    };

    Ok(Sinad {
        sinad_db,
        snr_db,
        thd_db,
        sfdr_db,
        enob: enob_from_sinad(sinad_db),
        signal_bins: bins,
        signal_power,
        noise_power,
    })
}

/// SINAD for a record whose excitation is at the given *frequencies*.
///
/// Converts each to its nearest bin at the record's own resolution
/// `fs / n`. A frequency that is not an exact multiple of that resolution
/// is not coherently sampled and its energy will smear -- pass `leak` to
/// absorb it, or generate the record on the bin grid to begin with.
pub fn sinad_at_freqs(
    x: &[f64],
    fs: f64,
    freqs: &[f64],
    leak: usize,
) -> Result<Sinad, NumericError> {
    if !(fs > 0.0) {
        return Err(NumericError::Decomposition(
            "sinad: the sample rate must be positive".into(),
        ));
    }
    let df = fs / x.len() as f64;
    let bins: Vec<usize> = freqs
        .iter()
        .filter(|f| **f > 0.0 && **f < fs / 2.0)
        .map(|f| (f / df).round() as usize)
        .collect();
    if !freqs.is_empty() && bins.is_empty() {
        return Err(NumericError::Decomposition(
            "sinad: every given frequency is outside (0, fs/2)".into(),
        ));
    }
    sinad_at_bins(x, &bins, leak)
}

/// The SINAD an ideal N-bit converter would show for a signal of this
/// crest factor -- the estimate, as opposed to the measurement above.
///
/// The full-scale sine figure `6.02 N + 1.76` assumes the signal uses the
/// whole range and has a crest factor of sqrt(2). A signal with a higher
/// crest factor must be backed off to fit its peaks, so its RMS -- and
/// therefore its SINAD -- drops by exactly the ratio of the two crest
/// factors:
///
/// ```text
/// SINAD ~= 6.02 N + 1.76 - 20 log10(CF / sqrt(2))
/// ```
///
/// This is what makes crest factor a converter-resolution problem rather
/// than an aesthetic one, and it is the estimate the measured `sinad_*`
/// functions above exist to be checked against.
pub fn sinad_estimate(bits: f64, crest_factor: f64) -> Option<f64> {
    if !(crest_factor > 0.0) || !bits.is_finite() {
        return None;
    }
    Some(6.02 * bits + 1.76 - 20.0 * (crest_factor / std::f64::consts::SQRT_2).log10())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::TAU;

    fn tone(n: usize, bin: usize, amp: f64) -> Vec<f64> {
        (0..n)
            .map(|i| amp * (TAU * bin as f64 * i as f64 / n as f64).sin())
            .collect()
    }

    /// A pure tone on an exact bin has nothing in it but the tone, so the
    /// only "noise" is the FFT's own round-off -- around 1e-32 of the
    /// signal, i.e. 300-odd dB down. Not infinite (the arithmetic is not
    /// exact), but far beyond anything a real converter reaches, which is
    /// the property worth pinning: the measurement must not invent a
    /// noise floor where the record has none.
    #[test]
    fn a_clean_tone_has_essentially_no_noise_floor() {
        let x = tone(1024, 64, 1.0);
        let r = sinad_at_bins(&x, &[], 0).unwrap();
        assert_eq!(r.signal_bins, vec![64], "the fundamental should be found without being named");
        assert!(r.sinad_db > 200.0, "got {} dB, expected round-off only", r.sinad_db);
    }

    /// The textbook case: a full-scale sine quantized to N bits should
    /// measure back as N bits, within the spread of a single record.
    #[test]
    fn a_quantized_sine_measures_back_its_own_bit_depth() {
        for bits in [8u32, 10, 12] {
            let n = 4096;
            let x = tone(n, 127, 1.0);
            let levels = (1u32 << (bits - 1)) as f64;
            let q: Vec<f64> = x.iter().map(|v| (v * levels).round() / levels).collect();
            let r = sinad_at_bins(&q, &[127], 0).unwrap();
            let err = (r.enob - bits as f64).abs();
            assert!(
                err < 0.6,
                "{bits}-bit sine measured {:.2} ENOB ({:.1} dB SINAD)",
                r.enob,
                r.sinad_db
            );
        }
    }

    /// The reason the excitation set is a parameter: treating a multisine
    /// as a single tone counts every other tone as noise, and the answer
    /// comes out tens of dB too low.
    #[test]
    fn a_multisine_needs_all_its_tones_or_the_answer_is_nonsense() {
        let n = 2048;
        let bins = [30usize, 61, 97, 143];
        let mut x = vec![0.0; n];
        for (k, &b) in bins.iter().enumerate() {
            let phase = k as f64 * 0.7;
            for (i, v) in x.iter_mut().enumerate() {
                *v += (TAU * b as f64 * i as f64 / n as f64 + phase).sin();
            }
        }
        // Quantize so there is a real noise floor to measure.
        let q: Vec<f64> = x.iter().map(|v| (v * 512.0).round() / 512.0).collect();

        let all = sinad_at_bins(&q, &bins, 0).unwrap();
        let one = sinad_at_bins(&q, &[bins[0]], 0).unwrap();
        assert!(
            all.sinad_db > one.sinad_db + 4.0,
            "counting the other tones as noise should cost a lot: all={:.1} dB, one={:.1} dB",
            all.sinad_db,
            one.sinad_db
        );
    }

    /// Distortion must land in SINAD but not in SNR -- that difference is
    /// the entire reason both are reported.
    #[test]
    fn harmonic_distortion_separates_sinad_from_snr() {
        let n = 4096;
        let f0 = 100;
        // Quantized, so there is a genuine noise floor for SNR to be
        // *about*. Without one the record has distortion and nothing else,
        // SNR is then infinite, and the comparison the test is making does
        // not exist.
        let x: Vec<f64> = (0..n)
            .map(|i| {
                let t = TAU * i as f64 / n as f64;
                let v = (f0 as f64 * t).sin() + 0.02 * (2.0 * f0 as f64 * t).sin();
                (v * 4096.0).round() / 4096.0
            })
            .collect();
        let r = sinad_at_bins(&x, &[f0], 0).unwrap();
        let snr = r.snr_db.expect("a single-tone record should report SNR");
        assert!(
            snr > r.sinad_db + 10.0,
            "SNR should ignore the harmonic SINAD counts: sinad={:.1}, snr={:.1}",
            r.sinad_db,
            snr
        );
        let thd = r.thd_db.expect("a second harmonic should register as THD");
        // -34 dB is 0.02 in amplitude, expressed as power.
        assert!((thd - 20.0 * 0.02f64.log10()).abs() < 1.0, "thd {thd:.1} dB");
    }

    /// The crest-factor estimate is the paper's own claim: every doubling
    /// of crest factor costs 6 dB, which is one bit.
    #[test]
    fn the_estimate_costs_one_bit_per_doubling_of_crest_factor() {
        let a = sinad_estimate(12.0, std::f64::consts::SQRT_2).unwrap();
        let b = sinad_estimate(12.0, 2.0 * std::f64::consts::SQRT_2).unwrap();
        assert!((a - (6.02 * 12.0 + 1.76)).abs() < 1e-9, "a sqrt(2) crest is the reference");
        assert!(((a - b) - 6.0206).abs() < 1e-3, "doubling should cost ~6.02 dB, got {:.4}", a - b);
        assert!((enob_from_sinad(a) - enob_from_sinad(b) - 1.0).abs() < 1e-3);
    }

    #[test]
    fn rejects_input_it_cannot_measure() {
        assert!(sinad_at_bins(&[1.0, 2.0], &[], 0).is_err());
        let x = tone(256, 8, 1.0);
        assert!(sinad_at_bins(&x, &[0], 0).is_err(), "bin 0 is DC and is not excitation");
        assert!(sinad_at_freqs(&x, -1.0, &[10.0], 0).is_err());
    }

    /// Frequencies are converted at the record's own resolution, so
    /// naming a tone in Hz must find the same bin as naming it directly.
    #[test]
    fn frequencies_resolve_to_the_same_bins_as_indices() {
        let n = 1024;
        let fs = 1024.0; // 1 Hz per bin
        let x = tone(n, 40, 1.0);
        let by_freq = sinad_at_freqs(&x, fs, &[40.0], 0).unwrap();
        assert_eq!(by_freq.signal_bins, vec![40]);
    }
}
