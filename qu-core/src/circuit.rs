//! Equivalent-circuit elements and their composition (§48).
//!
//! An impedance is a complex function of angular frequency. This module is
//! the element set and the two ways of combining them -- series and parallel
//! -- so a circuit can be built as a value and evaluated, rather than each
//! caller writing out the algebraically pre-solved closed form by hand.
//!
//! That hand-solving is the thing this replaces. The Randles fit in
//! `benchmarks/matty_suite/bench_impedance_fit.qu:129` reads
//!
//! ```text
//! randles(freqs, Rs, Rct, Cdl) := Rs + Rct ./ (1 + j .* (2*pi*freqs) .* Rct .* Cdl)
//! ```
//!
//! which is `Rs + (Rct | Cdl)` with the parallel branch solved on paper. That
//! is fine for one circuit and does not survive a library of them.
//!
//! **Conventions are taken from `papers/ecm-pf`**, which has a Qu and a Python
//! implementation agreeing to 6.1e-16 across 40 topologies. Matching it
//! exactly is deliberate: it makes that suite usable as a conformance check
//! on this code rather than something to be re-derived.
//!
//! | Kind | Element | Impedance | Params |
//! |---|---|---|---|
//! | `R` | resistor | `R` | 1 |
//! | `C` | capacitor | `1/(jwC)` | 1 |
//! | `L` | inductor | `jwL` | 1 |
//! | `Q` | CPE | `1/(Q (jw)^n)` | 2 |
//! | `W` | Warburg (semi-infinite) | `Aw (1-j)/sqrt(w)` | 1 |
//! | `Wo` | finite-length, open | `Rw coth(x)/x`, `x = sqrt(jwT)` | 2 |
//! | `Ws` | finite-length, short | `Rw tanh(x)/x`, `x = sqrt(jwT)` | 2 |
//! | `G` | Gerischer | `Zg/sqrt(k + jw)` | 2 |
//! | `P` | porous (de Levie) | `sqrt(Rp Zi)/tanh(sqrt(Rp/Zi))`, `Zi = 1/(Q (jw)^n)` | 3 |
//! | `H` | Havriliak-Negami | `Rh/(1 + (jw tau)^a)^g` | 4 |
//!
//! `Ws`/`Wo` match §48's prose (Ahmed's ruling, 2026-09-16). `T`/`O` were the
//! original single letters and are still READ as deprecated aliases by the
//! spec-string parser, but never written -- so a round trip normalises them
//! and the deprecation actually retires rather than persisting forever.
//! Element names are matched LONGEST-FIRST for this to be possible at all:
//! `Ws` was already a valid spelling before the rename (a `W` with instance
//! tag `s`), so the boundary between element and tag could no longer be
//! "one character".
//!
//! **Parameter order is left-to-right leaf order**, and that is the single
//! documented source of truth every consumer indexes against. Fixing it here
//! costs nothing; discovering later that two consumers disagree is a
//! migration.

use crate::Complex64;

/// `z^n` for real `n`, via polar form.
///
/// Not `exp(n ln z)`: the polar route is exact for the `(jw)^n` that every
/// CPE-family element needs, and avoids `ln`'s branch cut sitting on the
/// negative real axis where `jw` lands for negative `w`.
fn powf(z: Complex64, n: f64) -> Complex64 {
    let r = z.magnitude();
    if r == 0.0 {
        // 0^0 is 1 by the usual convention; 0^positive is 0. A negative
        // exponent here is a genuine pole and yields infinity rather than
        // a silent zero.
        return if n == 0.0 {
            Complex64::real(1.0)
        } else if n > 0.0 {
            Complex64::real(0.0)
        } else {
            Complex64::real(f64::INFINITY)
        };
    }
    Complex64::from_polar(r.powf(n), z.arg() * n)
}

/// Complex hyperbolic tangent, saturating rather than overflowing.
///
/// `tanh(a + bi) = (sinh 2a + i sin 2b) / (cosh 2a + cos 2b)`.
///
/// Past `|2a| = 40` the denominator's `cosh` stops being representable and
/// the direct formula returns NaN. The true value there is exactly
/// `sign(a)`, to far better than f64 precision, so saturating changes no
/// value that was computable and supplies the right one where it was not.
///
/// **This branch is reached in ordinary use, not at an edge**: the
/// finite-length Warburg and porous elements evaluate `tanh(sqrt(jwT))`
/// across nine decades of frequency, and a happy-path test will not find it.
/// That is not hypothetical -- in `papers/ecm-pf`'s cross-language check,
/// circuits 1-37 agreed and 38 failed, 38 being the first containing a
/// finite-length Warburg and so the first to call `tanh` on a complex
/// argument at all. Thirty-seven agreements said nothing about the
/// thirty-eighth.
pub fn tanh(z: Complex64) -> Complex64 {
    let (a2, b2) = (2.0 * z.re, 2.0 * z.im);
    if a2.abs() > 40.0 {
        return Complex64::real(if a2 > 0.0 { 1.0 } else { -1.0 });
    }
    let den = a2.cosh() + b2.cos();
    if den == 0.0 {
        // tanh has poles at a = 0, b = (k + 1/2) pi.
        return Complex64::real(f64::INFINITY);
    }
    Complex64 {
        re: a2.sinh() / den,
        im: b2.sin() / den,
    }
}

/// `coth(z) = 1/tanh(z)`, with the same saturation behaviour.
pub fn coth(z: Complex64) -> Complex64 {
    Complex64::real(1.0).div(tanh(z))
}

/// One circuit element.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Element {
    /// `R`
    Resistor(f64),
    /// `C`
    Capacitor(f64),
    /// `L`
    Inductor(f64),
    /// `Q` -- constant phase element. `n = 1` is a capacitor `Q`, `n = 0` a
    /// resistor `1/Q`, `n = 0.5` a Warburg.
    Cpe { q: f64, n: f64 },
    /// `W` -- semi-infinite Warburg diffusion, a constant -45 degree phase.
    Warburg(f64),
    /// `O` -- finite-length diffusion with a reflecting (open) boundary.
    FiniteOpen { rw: f64, tau: f64 },
    /// `T` -- finite-length diffusion with a transmissive (short) boundary.
    FiniteShort { rw: f64, tau: f64 },
    /// `G` -- Gerischer, for a coupled chemical reaction.
    Gerischer { zg: f64, k: f64 },
    /// `P` -- de Levie porous electrode, whose pore wall is a CPE.
    Porous { rp: f64, q: f64, n: f64 },
    /// `H` -- Havriliak-Negami relaxation.
    HavriliakNegami { rh: f64, tau: f64, a: f64, g: f64 },
}

impl Element {
    /// How many scalar parameters this element carries, for the flat
    /// left-to-right parameter vector.
    pub fn nparam(&self) -> usize {
        match self {
            Element::Resistor(_) | Element::Capacitor(_) | Element::Inductor(_) => 1,
            Element::Warburg(_) => 1,
            Element::Cpe { .. }
            | Element::FiniteOpen { .. }
            | Element::FiniteShort { .. }
            | Element::Gerischer { .. } => 2,
            Element::Porous { .. } => 3,
            Element::HavriliakNegami { .. } => 4,
        }
    }

    /// The canonical one- or two-letter code for this element, the same one
    /// the spec-string grammar uses.
    pub fn code(&self) -> &'static str {
        match self {
            Element::Resistor(_) => "R",
            Element::Capacitor(_) => "C",
            Element::Inductor(_) => "L",
            Element::Warburg(_) => "W",
            Element::Cpe { .. } => "Q",
            Element::FiniteOpen { .. } => "Wo",
            Element::FiniteShort { .. } => "Ws",
            Element::Gerischer { .. } => "G",
            Element::Porous { .. } => "P",
            Element::HavriliakNegami { .. } => "H",
        }
    }

    /// Per-parameter `(suffix, role)`, in the flat vector's order.
    ///
    /// The suffix is the name the struct field already carries, so a fitted
    /// `Q1_n` is findable in this file without a translation table. A
    /// one-parameter element has an empty suffix: there is nothing to
    /// disambiguate, and `R1` reads better than `R1_r`.
    pub fn param_meta(&self) -> &'static [(&'static str, ParamRole)] {
        use ParamRole::{Exponent, Magnitude};
        match self {
            Element::Resistor(_)
            | Element::Capacitor(_)
            | Element::Inductor(_)
            | Element::Warburg(_) => &[("", Magnitude)],
            Element::Cpe { .. } => &[("q", Magnitude), ("n", Exponent)],
            Element::FiniteOpen { .. } | Element::FiniteShort { .. } => {
                &[("rw", Magnitude), ("tau", Magnitude)]
            }
            Element::Gerischer { .. } => &[("zg", Magnitude), ("k", Magnitude)],
            Element::Porous { .. } => &[("rp", Magnitude), ("q", Magnitude), ("n", Exponent)],
            Element::HavriliakNegami { .. } => &[
                ("rh", Magnitude),
                ("tau", Magnitude),
                ("a", Exponent),
                ("g", Exponent),
            ],
        }
    }

    /// The same element with new parameter values, `p` in the flat vector's
    /// order. Topology is preserved; only the numbers change.
    ///
    /// Panics if `p` is shorter than `nparam()`. Every caller in-tree slices
    /// it out of a vector whose length was already checked against
    /// [`Circuit::nparam`], and returning a `Result` here would put an
    /// unreachable error branch inside the fitter's inner loop.
    pub fn with_params(&self, p: &[f64]) -> Element {
        match self {
            Element::Resistor(_) => Element::Resistor(p[0]),
            Element::Capacitor(_) => Element::Capacitor(p[0]),
            Element::Inductor(_) => Element::Inductor(p[0]),
            Element::Warburg(_) => Element::Warburg(p[0]),
            Element::Cpe { .. } => Element::Cpe { q: p[0], n: p[1] },
            Element::FiniteOpen { .. } => Element::FiniteOpen {
                rw: p[0],
                tau: p[1],
            },
            Element::FiniteShort { .. } => Element::FiniteShort {
                rw: p[0],
                tau: p[1],
            },
            Element::Gerischer { .. } => Element::Gerischer { zg: p[0], k: p[1] },
            Element::Porous { .. } => Element::Porous {
                rp: p[0],
                q: p[1],
                n: p[2],
            },
            Element::HavriliakNegami { .. } => Element::HavriliakNegami {
                rh: p[0],
                tau: p[1],
                a: p[2],
                g: p[3],
            },
        }
    }

    /// Impedance at angular frequency `w` (rad/s).
    pub fn impedance(&self, w: f64) -> Complex64 {
        let jw = Complex64 { re: 0.0, im: w };
        let one = Complex64::real(1.0);
        match *self {
            Element::Resistor(r) => Complex64::real(r),
            Element::Capacitor(c) => one.div(jw.scale(c)),
            Element::Inductor(l) => jw.scale(l),
            Element::Cpe { q, n } => one.div(powf(jw, n).scale(q)),
            // (1 - j)/sqrt(w): the -45 degree phase that makes a Warburg
            // recognisable on a Nyquist plot as a straight line.
            Element::Warburg(aw) => Complex64 { re: 1.0, im: -1.0 }.scale(aw / w.sqrt()),
            Element::FiniteOpen { rw, tau } => {
                let x = jw.scale(tau).sqrt();
                coth(x).div(x).scale(rw)
            }
            Element::FiniteShort { rw, tau } => {
                let x = jw.scale(tau).sqrt();
                tanh(x).div(x).scale(rw)
            }
            Element::Gerischer { zg, k } => {
                Complex64::real(zg).div(Complex64 { re: k, im: w }.sqrt())
            }
            Element::Porous { rp, q, n } => {
                let zi = one.div(powf(jw, n).scale(q));
                let x = Complex64::real(rp).div(zi).sqrt();
                zi.scale(rp).sqrt().div(tanh(x))
            }
            Element::HavriliakNegami { rh, tau, a, g } => {
                let inner = one.add(powf(jw.scale(tau), a));
                Complex64::real(rh).div(powf(inner, g))
            }
        }
    }
}

/// What a scalar parameter *is*, which is the only thing a fitter needs to
/// know about it beyond its current value.
///
/// A magnitude (`R`, `C`, `Aw`, `tau`, ...) spans decades: the capacitances
/// in one cell can be 1e-9 and the resistances 1e3, and a step that moves
/// the resistance sensibly moves the capacitance by a million times its own
/// value. Those are fitted as `log10` of themselves so one step size is
/// right for every parameter, which is also what keeps them positive without
/// a bound doing the work. An exponent (`n`, `a`, `g`) is already O(1) and
/// lives in `(0, 1]`, so logging it would fight the fit instead of helping.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParamRole {
    /// Positive, scale-free, fitted in log space.
    Magnitude,
    /// Dimensionless exponent near 1, fitted directly.
    Exponent,
}

/// A circuit: a leaf element, or elements combined in series or parallel.
#[derive(Clone, Debug, PartialEq)]
pub enum Circuit {
    Leaf(Element),
    /// Impedances add.
    Series(Vec<Circuit>),
    /// Admittances add.
    Parallel(Vec<Circuit>),
}

impl Circuit {
    /// Impedance at angular frequency `w` (rad/s).
    pub fn impedance(&self, w: f64) -> Complex64 {
        match self {
            Circuit::Leaf(e) => e.impedance(w),
            Circuit::Series(parts) => parts
                .iter()
                .fold(Complex64::real(0.0), |acc, p| acc.add(p.impedance(w))),
            Circuit::Parallel(parts) => {
                // Sum admittances. A branch of zero impedance (an ideal
                // short) makes the whole parallel block zero, and is handled
                // here rather than by letting 1/0 produce an infinity that
                // then has to survive a sum.
                let mut y = Complex64::real(0.0);
                for p in parts {
                    let z = p.impedance(w);
                    if z.magnitude() == 0.0 {
                        return Complex64::real(0.0);
                    }
                    y = y.add(Complex64::real(1.0).div(z));
                }
                if y.magnitude() == 0.0 {
                    // Every branch an ideal open.
                    return Complex64::real(f64::INFINITY);
                }
                Complex64::real(1.0).div(y)
            }
        }
    }

    /// Impedance over a vector of frequencies in **Hz** (not rad/s), which is
    /// the axis measured data actually carries.
    pub fn spectrum_hz(&self, freqs_hz: &[f64]) -> Vec<Complex64> {
        freqs_hz
            .iter()
            .map(|f| self.impedance(std::f64::consts::TAU * f))
            .collect()
    }

    /// Total parameter count, left-to-right leaf order.
    pub fn nparam(&self) -> usize {
        match self {
            Circuit::Leaf(e) => e.nparam(),
            Circuit::Series(parts) | Circuit::Parallel(parts) => {
                parts.iter().map(|p| p.nparam()).sum()
            }
        }
    }

    /// Every leaf element, in the flat parameter vector's own left-to-right
    /// order, so the two can be walked together.
    pub fn leaves(&self) -> Vec<&Element> {
        let mut out = Vec::new();
        self.collect_leaves(&mut out);
        out
    }

    fn collect_leaves<'a>(&'a self, out: &mut Vec<&'a Element>) {
        match self {
            Circuit::Leaf(e) => out.push(e),
            Circuit::Series(parts) | Circuit::Parallel(parts) => {
                for p in parts {
                    p.collect_leaves(out);
                }
            }
        }
    }

    /// A name per parameter, in the flat vector's order: the element code,
    /// its 1-based occurrence among elements of that code, and the field
    /// suffix when the element has more than one parameter -- `R1`, `R2`,
    /// `Q1_q`, `Q1_n`.
    ///
    /// Instance tags from a spec string (`Rct`) cannot be used for this: the
    /// parser discards them and `circuit(...)` built from `series`/`parallel`
    /// never had any. Counting occurrences gives every circuit names, from
    /// either construction route, and gives the SAME names for the same
    /// topology -- which is what a fit report needs, since the reader has to
    /// match a number against a position in the spec string.
    pub fn param_names(&self) -> Vec<String> {
        let mut seen: Vec<(&'static str, usize)> = Vec::new();
        let mut out = Vec::with_capacity(self.nparam());
        for e in self.leaves() {
            let code = e.code();
            let idx = match seen.iter_mut().find(|(c, _)| *c == code) {
                Some((_, n)) => {
                    *n += 1;
                    *n
                }
                None => {
                    seen.push((code, 1));
                    1
                }
            };
            for (suffix, _) in e.param_meta() {
                if suffix.is_empty() {
                    out.push(format!("{code}{idx}"));
                } else {
                    out.push(format!("{code}{idx}_{suffix}"));
                }
            }
        }
        out
    }

    /// The role of each parameter, in the flat vector's order.
    pub fn param_roles(&self) -> Vec<ParamRole> {
        self.leaves()
            .iter()
            .flat_map(|e| e.param_meta().iter().map(|(_, r)| *r))
            .collect()
    }

    /// The same topology with new parameter values, consumed in left-to-right
    /// leaf order -- the rebuild step of every iteration of a fit.
    ///
    /// A wrong length is an error in BOTH directions, matching
    /// `circuit(spec, params)`: too many is rejected rather than truncated,
    /// because silently ignoring the tail would fit a different circuit than
    /// the one written.
    pub fn with_params(&self, params: &[f64]) -> Result<Circuit, String> {
        let want = self.nparam();
        if params.len() != want {
            return Err(format!(
                "circuit has {want} parameter(s) but {} were given",
                params.len()
            ));
        }
        let mut at = 0usize;
        Ok(self.rebuild(params, &mut at))
    }

    fn rebuild(&self, params: &[f64], at: &mut usize) -> Circuit {
        match self {
            Circuit::Leaf(e) => {
                let n = e.nparam();
                let slice = &params[*at..*at + n];
                *at += n;
                Circuit::Leaf(e.with_params(slice))
            }
            Circuit::Series(parts) => {
                Circuit::Series(parts.iter().map(|p| p.rebuild(params, at)).collect())
            }
            Circuit::Parallel(parts) => {
                Circuit::Parallel(parts.iter().map(|p| p.rebuild(params, at)).collect())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: f64 = 10.0;

    /// RELATIVE closeness, scaled by the larger magnitude.
    ///
    /// An absolute tolerance is the wrong standard for impedance: these
    /// values span 1e-3 to 1e9 ohms across an ordinary sweep, so a fixed
    /// epsilon is either meaninglessly loose at one end or unsatisfiable at
    /// the other. The first draft of this file used `1e-12` absolute and
    /// failed a CPE identity that was correct to 1e-16 RELATIVE, on a
    /// quantity of order 1e5 -- the test was wrong, not the code.
    fn close(a: Complex64, b: Complex64, rel: f64) -> bool {
        let scale = a.magnitude().max(b.magnitude()).max(1e-300);
        (a.sub(b)).magnitude() / scale < rel
    }

    #[test]
    fn the_three_ideal_elements_are_their_textbook_impedances() {
        assert!(close(Element::Resistor(50.0).impedance(W), Complex64::real(50.0), 1e-15));
        // A capacitor is -j/(wC); an inductor is +jwL. The SIGNS are the
        // whole content of the test -- a swapped one still produces a
        // plausible Nyquist plot, mirrored.
        let c = Element::Capacitor(1e-6).impedance(W);
        assert!(c.re.abs() < 1e-15 && (c.im + 1.0 / (W * 1e-6)).abs() < 1e-9, "{c:?}");
        let l = Element::Inductor(1e-3).impedance(W);
        assert!(l.re.abs() < 1e-15 && (l.im - W * 1e-3).abs() < 1e-15, "{l:?}");
    }

    #[test]
    fn a_cpe_degenerates_to_the_elements_it_interpolates() {
        // The reason CPE is the element worth getting right: n=1 IS a
        // capacitor, n=0 IS a resistor, n=0.5 IS a Warburg. If the (jw)^n
        // branch is wrong, these three stop agreeing.
        let cap = Element::Capacitor(1e-6).impedance(W);
        let cpe1 = Element::Cpe { q: 1e-6, n: 1.0 }.impedance(W);
        assert!(close(cap, cpe1, 1e-14), "n=1 must be a capacitor: {cpe1:?} vs {cap:?}");

        let res = Element::Resistor(1.0 / 1e-6).impedance(W);
        let cpe0 = Element::Cpe { q: 1e-6, n: 0.0 }.impedance(W);
        assert!(close(res, cpe0, 1e-14), "n=0 must be a resistor: {cpe0:?}");

        let warb = Element::Warburg(1.0).impedance(W);
        let cpe_half = Element::Cpe { q: 1.0 / 2f64.sqrt(), n: 0.5 }.impedance(W);
        assert!(
            (warb.arg() - cpe_half.arg()).abs() < 1e-12,
            "n=0.5 must share the Warburg phase: {} vs {}",
            cpe_half.arg(),
            warb.arg()
        );
    }

    #[test]
    fn a_warburg_sits_at_minus_forty_five_degrees_at_every_frequency() {
        // The defining property, and scale-free: it holds at every w, which
        // is what makes it a straight line on a Nyquist plot.
        for w in [1e-3, 1.0, 1e3, 1e6] {
            let z = Element::Warburg(2.5).impedance(w);
            assert!(
                (z.arg() + std::f64::consts::FRAC_PI_4).abs() < 1e-14,
                "w={w}: arg={} expected -pi/4",
                z.arg()
            );
        }
    }

    #[test]
    fn finite_length_diffusion_hits_its_two_limits() {
        let (rw, tau) = (10.0, 1.0);
        // Short (transmissive): as w -> 0, tanh(x)/x -> 1, so Z -> Rw, real.
        let lo = Element::FiniteShort { rw, tau }.impedance(1e-12);
        assert!((lo.re - rw).abs() < 1e-5 && lo.im.abs() < 1e-4, "{lo:?}");
        // and vanishes at high frequency.
        let hi = Element::FiniteShort { rw, tau }.impedance(1e12);
        assert!(hi.magnitude() < 1e-4, "{hi:?}");
        // Open (reflecting): as w -> 0, coth(x)/x -> 1/x^2 = 1/(jw tau),
        // i.e. it becomes a capacitor and diverges.
        let open_lo = Element::FiniteOpen { rw, tau }.impedance(1e-9);
        assert!(open_lo.magnitude() > 1e8, "open boundary must diverge: {open_lo:?}");
    }

    #[test]
    fn tanh_saturates_instead_of_returning_nan() {
        // THE branch a happy-path test misses. Nine decades of frequency
        // reach it routinely; the direct formula overflows cosh and yields
        // NaN, which then propagates through the whole spectrum silently.
        let big = tanh(Complex64 { re: 30.0, im: 0.3 });
        assert!(big.re.is_finite() && big.im.is_finite(), "{big:?}");
        assert!((big.re - 1.0).abs() < 1e-12 && big.im.abs() < 1e-12, "{big:?}");
        let neg = tanh(Complex64 { re: -30.0, im: 0.3 });
        assert!((neg.re + 1.0).abs() < 1e-12, "{neg:?}");

        // And the element that reaches it must stay finite across the sweep.
        for w in [1e-6, 1.0, 1e6, 1e12] {
            let z = Element::FiniteShort { rw: 10.0, tau: 1.0 }.impedance(w);
            assert!(z.re.is_finite() && z.im.is_finite(), "w={w}: {z:?}");
        }
    }

    #[test]
    fn series_adds_impedance_and_parallel_adds_admittance() {
        let r = |v| Circuit::Leaf(Element::Resistor(v));
        let s = Circuit::Series(vec![r(10.0), r(30.0)]).impedance(W);
        assert!(close(s, Complex64::real(40.0), 1e-14), "{s:?}");
        let p = Circuit::Parallel(vec![r(10.0), r(10.0)]).impedance(W);
        assert!(close(p, Complex64::real(5.0), 1e-14), "{p:?}");
        // Unequal, to catch a formula that only works for equal branches.
        let p2 = Circuit::Parallel(vec![r(10.0), r(40.0)]).impedance(W);
        assert!(close(p2, Complex64::real(8.0), 1e-14), "{p2:?}");
    }

    #[test]
    fn a_randles_circuit_reproduces_the_hand_solved_closed_form() {
        // The point of the whole module. `bench_impedance_fit.qu:129` writes
        // the parallel branch pre-solved by hand:
        //     Rs + Rct / (1 + j w Rct Cdl)
        // Composing `Rs + (Rct | Cdl)` must agree with it exactly, at every
        // frequency -- that closed form is already validated against MATLAB
        // and Python, so agreeing with it is agreeing with them.
        let (rs, rct, cdl) = (0.2, 0.01, 100e-6);
        let built = Circuit::Series(vec![
            Circuit::Leaf(Element::Resistor(rs)),
            Circuit::Parallel(vec![
                Circuit::Leaf(Element::Resistor(rct)),
                Circuit::Leaf(Element::Capacitor(cdl)),
            ]),
        ]);
        for f in [1e-2, 1.0, 50.0, 1e3, 1e5] {
            let w = std::f64::consts::TAU * f;
            let z = built.impedance(w);
            let den = Complex64 { re: 1.0, im: w * rct * cdl };
            let closed = Complex64::real(rs).add(Complex64::real(rct).div(den));
            assert!(
                close(z, closed, 1e-14),
                "f={f}: composed {z:?} vs hand-solved {closed:?}"
            );
        }
    }

    #[test]
    fn parameter_count_follows_left_to_right_leaf_order() {
        let c = Circuit::Series(vec![
            Circuit::Leaf(Element::Resistor(1.0)),
            Circuit::Parallel(vec![
                Circuit::Leaf(Element::Cpe { q: 1e-5, n: 0.8 }),
                Circuit::Leaf(Element::Resistor(2.0)),
            ]),
            Circuit::Leaf(Element::HavriliakNegami { rh: 1.0, tau: 1.0, a: 0.9, g: 0.8 }),
        ]);
        assert_eq!(c.nparam(), 1 + 2 + 1 + 4);
    }

    #[test]
    fn an_ideal_short_in_parallel_shorts_the_block() {
        // 1/0 would otherwise produce an infinity that has to survive being
        // summed with finite admittances.
        let c = Circuit::Parallel(vec![
            Circuit::Leaf(Element::Resistor(0.0)),
            Circuit::Leaf(Element::Resistor(50.0)),
        ]);
        let z = c.impedance(W);
        assert!(z.magnitude() == 0.0, "{z:?}");
    }
}
