//! Reflection, matching, and one-port RF quantities.
//!
//! The Smith chart in the interpreter already computed a reflection
//! coefficient, inline and with the real and imaginary parts written as two
//! separate expressions -- so the one piece of arithmetic every quantity here
//! depends on existed only as a drawing step, was duplicated within that
//! step, and could not be called from a script. This module is that
//! arithmetic, once, as values.
//!
//! Conventions, stated because they are the ones that differ between texts:
//!
//!  * `z0` is a REAL reference impedance (50 ohm unless given). Complex-`z0`
//!    (power-wave) definitions exist and are NOT what this module computes;
//!    with a real `z0` the voltage-wave and power-wave definitions coincide,
//!    which is why the simpler one is safe here.
//!  * `vswr` of a perfect match is exactly 1, and of a total reflection is
//!    infinite. It is returned as `f64::INFINITY` rather than a large number
//!    or an error: infinity is the true value, it compares correctly, and it
//!    prints as `inf` rather than pretending to a precision it does not have.
//!  * `return_loss_db` is returned POSITIVE for a passive load (the usual
//!    engineering convention: "20 dB return loss" means well matched). A
//!    perfect match is infinite return loss.

use crate::Complex64;

/// Reflection coefficient of a load `z` against reference `z0`:
/// `Γ = (z - z0) / (z + z0)`.
///
/// `z = -z0` is the one pole: the denominator vanishes and `Γ` is genuinely
/// undefined, not merely large. That requires a negative real part, so it
/// cannot arise from a passive load, and it is reported rather than returned
/// as a NaN that would propagate silently into a plot.
pub fn reflection_coefficient(z: Complex64, z0: f64) -> Result<Complex64, String> {
    if z0 == 0.0 {
        return Err("z0 must be non-zero -- it is what the impedance is normalized by".into());
    }
    let (zr, zi) = (z.re / z0, z.im / z0);
    let den = (zr + 1.0) * (zr + 1.0) + zi * zi;
    if den == 0.0 {
        return Err(format!(
            "reflection coefficient is undefined at z = -z0 (z = {} {:+}j, z0 = {z0})",
            z.re, z.im
        ));
    }
    Ok(Complex64 {
        re: ((zr * zr - 1.0) + zi * zi) / den,
        im: (2.0 * zi) / den,
    })
}

/// The inverse: `z = z0 (1 + Γ) / (1 - Γ)`.
///
/// `Γ = 1` (an open) is the pole here, the mirror of `z = -z0` above.
pub fn impedance_from_reflection(g: Complex64, z0: f64) -> Result<Complex64, String> {
    let den = (1.0 - g.re) * (1.0 - g.re) + g.im * g.im;
    if den == 0.0 {
        return Err("impedance is undefined at gamma = 1 (an ideal open circuit)".into());
    }
    // (1+G)/(1-G) with the conjugate multiplied through.
    let num_re = 1.0 - (g.re * g.re + g.im * g.im);
    let num_im = 2.0 * g.im;
    Ok(Complex64 {
        re: z0 * num_re / den,
        im: z0 * num_im / den,
    })
}

/// Voltage standing-wave ratio from a reflection coefficient:
/// `(1 + |Γ|) / (1 - |Γ|)`.
///
/// `|Γ| >= 1` returns infinity rather than a negative number. A magnitude
/// above 1 means an active or mismeasured load; the ratio has no meaning
/// there, and a NEGATIVE vswr -- which the formula produces unguarded -- is
/// the kind of plausible-looking output that gets plotted without comment.
pub fn vswr(g: Complex64) -> f64 {
    let m = g.magnitude();
    if m >= 1.0 {
        f64::INFINITY
    } else {
        (1.0 + m) / (1.0 - m)
    }
}

/// Return loss in dB, positive for a passive load: `-20 log10 |Γ|`.
///
/// A perfect match (`Γ = 0`) is infinite return loss, not a division error.
pub fn return_loss_db(g: Complex64) -> f64 {
    let m = g.magnitude();
    if m == 0.0 {
        f64::INFINITY
    } else {
        -20.0 * m.log10()
    }
}

/// Mismatch loss in dB: the fraction of incident power not delivered,
/// `-10 log10(1 - |Γ|²)`. Zero for a perfect match.
pub fn mismatch_loss_db(g: Complex64) -> f64 {
    let m2 = g.magnitude().powi(2);
    if m2 >= 1.0 {
        f64::INFINITY
    } else {
        -10.0 * (1.0 - m2).log10()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(re: f64, im: f64) -> Complex64 {
        Complex64 { re, im }
    }

    #[test]
    fn a_matched_load_reflects_nothing() {
        let g = reflection_coefficient(c(50.0, 0.0), 50.0).unwrap();
        assert!(g.magnitude() < 1e-15, "got {g:?}");
        assert_eq!(vswr(g), 1.0, "a perfect match is exactly 1:1");
        assert!(return_loss_db(g).is_infinite());
        assert_eq!(mismatch_loss_db(g), 0.0);
    }

    #[test]
    fn a_short_and_an_open_sit_at_the_ends_of_the_real_axis() {
        // The two textbook endpoints of the Smith chart: a short is -1, an
        // open is +1. Both are total reflection, so both are infinite VSWR.
        let short = reflection_coefficient(c(0.0, 0.0), 50.0).unwrap();
        assert!((short.re + 1.0).abs() < 1e-15 && short.im.abs() < 1e-15, "{short:?}");
        assert!(vswr(short).is_infinite());
        assert!(return_loss_db(short).abs() < 1e-12, "0 dB return loss for a short");

        let open = reflection_coefficient(c(1e18, 0.0), 50.0).unwrap();
        assert!((open.re - 1.0).abs() < 1e-9, "{open:?}");
    }

    #[test]
    fn a_two_to_one_mismatch_is_the_textbook_case() {
        // 100 ohm on a 50 ohm line: |G| = 1/3, VSWR = 2, RL ~ 9.54 dB.
        let g = reflection_coefficient(c(100.0, 0.0), 50.0).unwrap();
        assert!((g.re - 1.0 / 3.0).abs() < 1e-15, "{g:?}");
        assert!((vswr(g) - 2.0).abs() < 1e-12);
        assert!((return_loss_db(g) - 9.542425094393249).abs() < 1e-9);
    }

    #[test]
    fn impedance_and_reflection_round_trip() {
        for z in [c(75.0, 25.0), c(10.0, -40.0), c(50.0, 0.0), c(0.0, 30.0)] {
            let g = reflection_coefficient(z, 50.0).unwrap();
            let back = impedance_from_reflection(g, 50.0).unwrap();
            assert!(
                (back.re - z.re).abs() < 1e-9 && (back.im - z.im).abs() < 1e-9,
                "{z:?} -> {g:?} -> {back:?}"
            );
        }
    }

    #[test]
    fn an_active_load_does_not_produce_a_negative_vswr() {
        // |G| > 1. Unguarded, (1+m)/(1-m) is NEGATIVE here -- a plausible
        // number with no physical meaning, which is worse than infinity.
        let g = c(1.5, 0.0);
        assert!(vswr(g).is_infinite(), "got {}", vswr(g));
        assert!(mismatch_loss_db(g).is_infinite());
    }

    #[test]
    fn the_poles_are_reported_rather_than_returned_as_nan() {
        assert!(reflection_coefficient(c(-50.0, 0.0), 50.0).is_err());
        assert!(impedance_from_reflection(c(1.0, 0.0), 50.0).is_err());
        assert!(reflection_coefficient(c(50.0, 0.0), 0.0).is_err());
    }
}
