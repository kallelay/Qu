//! Exact integer number theory on `f64`-carried integers: [`gcd`], [`lcm`],
//! [`primes_upto`], [`is_prime`], [`factor`], [`nchoosek`], [`factorial`].
//!
//! Qu's numbers are `f64`, so every function here first demands an INTEGER of
//! magnitude at most 2^53 (the range in which `f64` holds every integer
//! exactly) via [`to_int`], and says so in the error. Results that would not
//! be exact are errors, never rounded.

/// 2^53: the largest magnitude at which every integer is an exact `f64`.
pub const MAX_EXACT: f64 = 9_007_199_254_740_992.0;

/// Largest `n` accepted by [`primes_upto`] (one byte per odd number: ~50 MB,
/// and the `f64` result vector for 10^8 is about 40 MB).
pub const PRIMES_LIMIT: u64 = 100_000_000;

/// Validate that `x` is an integer with `|x| <= 2^53`.
pub fn to_int(who: &str, name: &str, x: f64) -> Result<i64, String> {
    if !x.is_finite() || x != x.trunc() {
        return Err(format!("{who}: {name} must be an integer, got {x}"));
    }
    if x.abs() > MAX_EXACT {
        return Err(format!(
            "{who}: {name} = {x} is beyond 2^53, where f64 stops representing every integer"
        ));
    }
    Ok(x as i64)
}

fn gcd_u(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        let t = a % b;
        a = b;
        b = t;
    }
    a
}

/// Greatest common divisor, non-negative; `gcd(0, 0) = 0`.
pub fn gcd(a: i64, b: i64) -> u64 {
    gcd_u(a.unsigned_abs(), b.unsigned_abs())
}

/// Least common multiple, non-negative; zero if either argument is zero.
/// Errors if the result exceeds 2^53.
pub fn lcm(a: i64, b: i64) -> Result<u64, String> {
    let (a, b) = (a.unsigned_abs(), b.unsigned_abs());
    if a == 0 || b == 0 {
        return Ok(0);
    }
    let g = gcd_u(a, b);
    let r = (a / g) as u128 * b as u128;
    if r > MAX_EXACT as u128 {
        return Err(format!("lcm: the result {r} is beyond 2^53 and not exactly representable"));
    }
    Ok(r as u64)
}

/// All primes `<= n`, ascending (odd-only sieve of Eratosthenes).
pub fn primes_upto(n: u64) -> Result<Vec<u64>, String> {
    if n > PRIMES_LIMIT {
        return Err(format!("primes: n = {n} is above the limit of {PRIMES_LIMIT}"));
    }
    if n < 2 {
        return Ok(Vec::new());
    }
    // index i represents the odd number 2i + 1 (i >= 1).
    let size = (n as usize - 1) / 2 + 1;
    let mut composite = vec![false; size];
    let mut i = 1usize;
    while (2 * i + 1) * (2 * i + 1) <= n as usize {
        if !composite[i] {
            let p = 2 * i + 1;
            let mut j = (p * p - 1) / 2;
            while j < size {
                composite[j] = true;
                j += p;
            }
        }
        i += 1;
    }
    let mut out = vec![2u64];
    out.extend((1..size).filter(|&i| !composite[i]).map(|i| (2 * i + 1) as u64));
    Ok(out)
}

fn mul_mod(a: u64, b: u64, m: u64) -> u64 {
    ((a as u128 * b as u128) % m as u128) as u64
}

fn pow_mod(mut b: u64, mut e: u64, m: u64) -> u64 {
    let mut r = 1u64 % m;
    b %= m;
    while e > 0 {
        if e & 1 == 1 {
            r = mul_mod(r, b, m);
        }
        b = mul_mod(b, b, m);
        e >>= 1;
    }
    r
}

/// Deterministic primality test (Miller-Rabin with the first twelve prime
/// bases, which is exact for every `n < 3.3e24`, so for all of `u64`).
pub fn is_prime(n: u64) -> bool {
    if n < 2 {
        return false;
    }
    const BASES: [u64; 12] = [2, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37];
    for &p in &BASES {
        if n == p {
            return true;
        }
        if n % p == 0 {
            return false;
        }
    }
    let mut d = n - 1;
    let mut s = 0;
    while d % 2 == 0 {
        d /= 2;
        s += 1;
    }
    'bases: for &a in &BASES {
        let mut x = pow_mod(a, d, n);
        if x == 1 || x == n - 1 {
            continue;
        }
        for _ in 0..s - 1 {
            x = mul_mod(x, x, n);
            if x == n - 1 {
                continue 'bases;
            }
        }
        return false;
    }
    true
}

/// Prime factors of `n >= 1` with multiplicity, ascending. `factor(1) = [1]`
/// (as MATLAB does). Trial division by 2, 3 and `6k +- 1` up to `sqrt(n)`,
/// cut short as soon as the cofactor is prime.
pub fn factor(n: u64) -> Result<Vec<u64>, String> {
    if n == 0 {
        return Err("factor: n must be a positive integer, got 0".to_string());
    }
    if n == 1 {
        return Ok(vec![1]);
    }
    let mut out = Vec::new();
    let mut m = n;
    for p in [2u64, 3] {
        while m % p == 0 {
            out.push(p);
            m /= p;
        }
    }
    let mut p = 5u64;
    let mut checked = false;
    while p * p <= m {
        if !checked && m > 1 << 20 {
            if is_prime(m) {
                break;
            }
            checked = true;
        }
        for q in [p, p + 2] {
            while m % q == 0 {
                out.push(q);
                m /= q;
                checked = false;
            }
        }
        p += 6;
    }
    if m > 1 {
        out.push(m);
    }
    Ok(out)
}

/// Binomial coefficient, exact. Errors if `k > n` or the result exceeds 2^53.
pub fn nchoosek(n: u64, k: u64) -> Result<u64, String> {
    if k > n {
        return Err(format!("nchoosek: k = {k} is larger than n = {n}"));
    }
    let k = k.min(n - k);
    let mut r: u128 = 1;
    for i in 1..=k as u128 {
        // r = C(n-k+i-1, i-1) -> C(n-k+i, i); the division is exact.
        r = r * (n as u128 - k as u128 + i) / i;
        if r > MAX_EXACT as u128 {
            return Err(format!(
                "nchoosek: C({n}, {}) is beyond 2^53 and not exactly representable",
                k.max(n - k)
            ));
        }
    }
    Ok(r as u64)
}

/// `n!`: exact (an integer-valued `f64`) for `n <= 18`; for `19..=170` the
/// correctly rounded product of the exact `f64` partial products, which is
/// not exact (`19! > 2^53`); above 170 it overflows `f64` and is an error.
pub fn factorial(n: u64) -> Result<f64, String> {
    if n > 170 {
        return Err(format!("factorial: {n}! overflows f64 (the largest is 170!)"));
    }
    let mut r = 1.0f64;
    for i in 2..=n {
        r *= i as f64;
    }
    Ok(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gcd_lcm_known() {
        assert_eq!(gcd(48, 18), 6);
        assert_eq!(gcd(-48, 18), 6);
        assert_eq!(gcd(0, 5), 5);
        assert_eq!(gcd(0, 0), 0);
        assert_eq!(lcm(4, 6).unwrap(), 12);
        assert_eq!(lcm(-4, 6).unwrap(), 12);
        assert_eq!(lcm(0, 6).unwrap(), 0);
        assert!(lcm(9_007_199_254_740_991, 9_007_199_254_740_990).is_err());
    }

    #[test]
    fn primes_known() {
        assert_eq!(primes_upto(30).unwrap(), vec![2, 3, 5, 7, 11, 13, 17, 19, 23, 29]);
        assert_eq!(primes_upto(2).unwrap(), vec![2]);
        assert_eq!(primes_upto(3).unwrap(), vec![2, 3]);
        assert!(primes_upto(1).unwrap().is_empty());
        assert!(primes_upto(0).unwrap().is_empty());
        assert_eq!(primes_upto(100).unwrap().len(), 25);
        assert_eq!(primes_upto(1_000_000).unwrap().len(), 78_498);
        assert!(primes_upto(PRIMES_LIMIT + 1).is_err());
    }

    #[test]
    fn miller_rabin_agrees_with_the_sieve_and_known_big_values() {
        let sieve = primes_upto(20_000).unwrap();
        for n in 0..=20_000u64 {
            assert_eq!(is_prime(n), sieve.binary_search(&n).is_ok(), "n = {n}");
        }
        // 2^53 - 111 is the largest prime below 2^53
        assert!(is_prime(9_007_199_254_740_881));
        assert!(!is_prime(9_007_199_254_740_991)); // 2^53 - 1 = 6361 * 69431 * 20394401
        assert!(!is_prime(3_215_031_751)); // strong pseudoprime to bases 2,3,5,7
    }

    #[test]
    fn factor_known() {
        assert_eq!(factor(360).unwrap(), vec![2, 2, 2, 3, 3, 5]);
        assert_eq!(factor(1).unwrap(), vec![1]);
        assert_eq!(factor(2).unwrap(), vec![2]);
        assert_eq!(factor(97).unwrap(), vec![97]);
        assert_eq!(factor(9_007_199_254_740_991).unwrap(), vec![6361, 69431, 20394401]);
        assert_eq!(factor(9_007_199_254_740_881).unwrap(), vec![9_007_199_254_740_881]);
        assert!(factor(0).is_err());
        for n in [12u64, 1001, 65536, 999_999_937 * 3, 600_851_475_143] {
            let f = factor(n).unwrap();
            assert_eq!(f.iter().product::<u64>(), n);
            assert!(f.iter().all(|&p| is_prime(p)));
        }
    }

    #[test]
    fn nchoosek_known() {
        assert_eq!(nchoosek(52, 5).unwrap(), 2_598_960);
        assert_eq!(nchoosek(5, 0).unwrap(), 1);
        assert_eq!(nchoosek(5, 5).unwrap(), 1);
        assert_eq!(nchoosek(0, 0).unwrap(), 1);
        assert_eq!(nchoosek(50, 25).unwrap(), 126_410_606_437_752);
    assert!(nchoosek(60, 30).is_err(), "1.18e17 is beyond 2^53");
    assert!(nchoosek(3, 4).is_err());
    }

    #[test]
    fn factorial_values() {
        assert_eq!(factorial(0).unwrap(), 1.0);
        assert_eq!(factorial(5).unwrap(), 120.0);
        assert_eq!(factorial(18).unwrap(), 6_402_373_705_728_000.0);
        assert!((factorial(170).unwrap() / 7.257415615307994e306 - 1.0).abs() < 1e-14);
        assert!(factorial(171).is_err());
    }
}
