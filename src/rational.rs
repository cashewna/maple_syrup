use anyhow::{Result, bail};
use std::cmp::Ordering;
use std::fmt;
use std::ops::{Add, Div, Mul, Neg, Sub};

/// Exact reduced rational number `num / den` with `den > 0` and `gcd(|num|, den) == 1`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Rational {
    pub num: i128,
    pub den: i128,
}

pub fn gcd_u128(mut a: u128, mut b: u128) -> u128 {
    while b != 0 {
        let r = a % b;
        a = b;
        b = r;
    }
    a
}

impl Rational {
    /// Construct a canonical reduced `Rational` from numerator and denominator.
    pub fn new(num: i128, den: i128) -> Result<Self> {
        if den == 0 {
            bail!("division by zero in Rational");
        }
        if num == 0 {
            return Ok(Self { num: 0, den: 1 });
        }
        let neg = (num < 0) ^ (den < 0);
        let g = gcd_u128(num.unsigned_abs(), den.unsigned_abs());
        let n_abs = num.unsigned_abs() / g;
        let d_abs = den.unsigned_abs() / g;
        let d_i128 =
            i128::try_from(d_abs).map_err(|_| anyhow::anyhow!("rational denominator overflow"))?;
        let n_i128 = if neg {
            if n_abs == (i128::MAX as u128) + 1 {
                i128::MIN
            } else {
                -(i128::try_from(n_abs)
                    .map_err(|_| anyhow::anyhow!("rational numerator overflow"))?)
            }
        } else {
            i128::try_from(n_abs).map_err(|_| anyhow::anyhow!("rational numerator overflow"))?
        };
        Ok(Self {
            num: n_i128,
            den: d_i128,
        })
    }

    pub const fn from_int(n: i128) -> Self {
        Self { num: n, den: 1 }
    }

    pub const fn zero() -> Self {
        Self { num: 0, den: 1 }
    }

    pub const fn one() -> Self {
        Self { num: 1, den: 1 }
    }

    pub const fn is_zero(self) -> bool {
        self.num == 0
    }

    pub const fn is_one(self) -> bool {
        self.num == 1 && self.den == 1
    }

    pub const fn is_neg_one(self) -> bool {
        self.num == -1 && self.den == 1
    }

    pub const fn is_integer(self) -> bool {
        self.den == 1
    }

    pub const fn is_negative(self) -> bool {
        self.num < 0
    }

    pub const fn is_positive(self) -> bool {
        self.num > 0
    }

    pub fn abs(self) -> Self {
        Self {
            num: self.num.saturating_abs(),
            den: self.den,
        }
    }

    pub fn recip(self) -> Result<Self> {
        Self::new(self.den, self.num)
    }

    pub fn checked_add(self, rhs: Self) -> Result<Self> {
        let g = gcd_u128(self.den.unsigned_abs(), rhs.den.unsigned_abs()) as i128;
        let d1 = self.den / g;
        let d2 = rhs.den / g;
        let num = self
            .num
            .checked_mul(d2)
            .and_then(|a| rhs.num.checked_mul(d1).and_then(|b| a.checked_add(b)))
            .ok_or_else(|| anyhow::anyhow!("rational addition overflow"))?;
        let den = self
            .den
            .checked_mul(d2)
            .ok_or_else(|| anyhow::anyhow!("rational denominator overflow"))?;
        Self::new(num, den)
    }

    pub fn checked_sub(self, rhs: Self) -> Result<Self> {
        let neg_rhs = rhs
            .num
            .checked_neg()
            .ok_or_else(|| anyhow::anyhow!("rational negation overflow"))?;
        self.checked_add(Self {
            num: neg_rhs,
            den: rhs.den,
        })
    }

    pub fn checked_mul(self, rhs: Self) -> Result<Self> {
        if self.is_zero() || rhs.is_zero() {
            return Ok(Self::zero());
        }
        let g1 = gcd_u128(self.num.unsigned_abs(), rhs.den.unsigned_abs()) as i128;
        let g2 = gcd_u128(rhs.num.unsigned_abs(), self.den.unsigned_abs()) as i128;
        let n = (self.num / g1)
            .checked_mul(rhs.num / g2)
            .ok_or_else(|| anyhow::anyhow!("rational multiplication overflow"))?;
        let d = (self.den / g2)
            .checked_mul(rhs.den / g1)
            .ok_or_else(|| anyhow::anyhow!("rational denominator overflow"))?;
        Self::new(n, d)
    }

    pub fn checked_div(self, rhs: Self) -> Result<Self> {
        if rhs.is_zero() {
            bail!("division by zero");
        }
        self.checked_mul(rhs.recip()?)
    }

    pub fn pow(self, exp: i32) -> Result<Self> {
        if exp == 0 {
            return Ok(Self::one());
        }
        let (base, abs_exp) = if exp < 0 {
            (self.recip()?, exp.unsigned_abs())
        } else {
            (self, exp as u32)
        };
        let n = base
            .num
            .checked_pow(abs_exp)
            .ok_or_else(|| anyhow::anyhow!("rational power overflow"))?;
        let d = base
            .den
            .checked_pow(abs_exp)
            .ok_or_else(|| anyhow::anyhow!("rational power overflow"))?;
        Self::new(n, d)
    }

    pub fn to_f64(self) -> f64 {
        self.num as f64 / self.den as f64
    }
}

impl Ord for Rational {
    fn cmp(&self, other: &Self) -> Ordering {
        if self == other {
            return Ordering::Equal;
        }
        // Reduce denominator cross-products using gcd(self.den, other.den) to minimize overflow.
        let g = gcd_u128(self.den.unsigned_abs(), other.den.unsigned_abs()) as i128;
        let lhs = self.num.checked_mul(other.den / g);
        let rhs = other.num.checked_mul(self.den / g);
        match (lhs, rhs) {
            (Some(a), Some(b)) => a.cmp(&b),
            _ => self
                .to_f64()
                .partial_cmp(&other.to_f64())
                .unwrap_or(Ordering::Equal),
        }
    }
}

impl PartialOrd for Rational {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for Rational {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.den == 1 {
            write!(f, "{}", self.num)
        } else {
            write!(f, "{}/{}", self.num, self.den)
        }
    }
}

impl Add for Rational {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        self.checked_add(rhs).expect("rational add overflow")
    }
}

impl Sub for Rational {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        self.checked_sub(rhs).expect("rational sub overflow")
    }
}

impl Mul for Rational {
    type Output = Self;
    fn mul(self, rhs: Self) -> Self {
        self.checked_mul(rhs).expect("rational mul overflow")
    }
}

impl Div for Rational {
    type Output = Self;
    fn div(self, rhs: Self) -> Self {
        self.checked_div(rhs).expect("rational div error")
    }
}

impl Neg for Rational {
    type Output = Self;
    fn neg(self) -> Self {
        Self {
            num: -self.num,
            den: self.den,
        }
    }
}
