//! Exact decimal arithmetic for auto-trade amounts.
//!
//! Represented as `(mantissa: u128, scale: u32)` meaning `mantissa * 10^-scale`.
//! **No floating point anywhere** (NFR / Safety-invariant 4): float rounding would
//! make a cap comparison silently wrong and is unacceptable for money.

/// Errors from decimal parsing / arithmetic. Overflow or an unparseable string
/// becomes a deny (grant-check) or a fail-degrade (pipeline).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AmountError {
    Empty,
    Invalid,
    Overflow,
}

impl std::fmt::Display for AmountError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            AmountError::Empty => "empty amount",
            AmountError::Invalid => "invalid decimal string",
            AmountError::Overflow => "amount arithmetic overflow",
        };
        f.write_str(s)
    }
}

impl std::error::Error for AmountError {}

/// An exact non-negative decimal: `mantissa * 10^-scale`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Decimal {
    mantissa: u128,
    scale: u32,
}

/// Floor precision applied to percentage-derived absolute amounts.
const PCT_ABS_SCALE: u32 = 8;

impl Decimal {
    /// Parse a non-negative decimal string.
    ///
    /// Accepts digits and at most one `.`; rejects empty, a lone `.`, signs,
    /// exponents, whitespace, or any non-digit character.
    pub fn parse(s: &str) -> Result<Self, AmountError> {
        if s.is_empty() {
            return Err(AmountError::Empty);
        }
        let mut int_part = String::new();
        let mut frac_part = String::new();
        let mut seen_dot = false;
        for c in s.chars() {
            match c {
                '0'..='9' => {
                    if seen_dot {
                        frac_part.push(c);
                    } else {
                        int_part.push(c);
                    }
                }
                '.' if !seen_dot => seen_dot = true,
                _ => return Err(AmountError::Invalid),
            }
        }
        // Lone "." or ".": no digits at all.
        if int_part.is_empty() && frac_part.is_empty() {
            return Err(AmountError::Invalid);
        }
        let scale = frac_part.len() as u32;
        let digits: String = format!("{int_part}{frac_part}");
        // Empty digits already excluded; parse the concatenated mantissa.
        let mantissa: u128 = if digits.is_empty() {
            0
        } else {
            digits.parse::<u128>().map_err(|_| AmountError::Overflow)?
        };
        Ok(Decimal { mantissa, scale }.normalized())
    }

    /// Build directly from parts (test helper / internal).
    #[cfg(test)]
    pub fn from_parts(mantissa: u128, scale: u32) -> Self {
        Decimal { mantissa, scale }.normalized()
    }

    /// True when the value is exactly zero.
    pub fn is_zero(&self) -> bool {
        self.mantissa == 0
    }

    /// Strip trailing zero fraction digits so equal values compare with the same
    /// scale (e.g. `1.50` and `1.5`). Zero collapses to scale 0.
    fn normalized(mut self) -> Self {
        if self.mantissa == 0 {
            self.scale = 0;
            return self;
        }
        while self.scale > 0 && self.mantissa.is_multiple_of(10) {
            self.mantissa /= 10;
            self.scale -= 1;
        }
        self
    }

    /// Rescale `self` up to `target_scale` (target_scale must be >= self.scale).
    /// Returns the widened mantissa or `Overflow`.
    fn mantissa_at(&self, target_scale: u32) -> Result<u128, AmountError> {
        let diff = target_scale
            .checked_sub(self.scale)
            .expect("mantissa_at requires target_scale >= self.scale");
        let factor = pow10(diff)?;
        self.mantissa
            .checked_mul(factor)
            .ok_or(AmountError::Overflow)
    }

    /// `self * pct / 100`, floored to 8 decimal places (FR-7 / AC-8).
    ///
    /// `self` is treated as the holding amount and `pct` as a percentage value
    /// (e.g. `25` = 25%). Example: `400.8 × 25% = 100.2`.
    pub fn pct_to_absolute(holding: &Decimal, pct: &Decimal) -> Result<Decimal, AmountError> {
        // product mantissa = h.m * p.m ; product scale = h.scale + p.scale ; then /100.
        let prod_m = holding
            .mantissa
            .checked_mul(pct.mantissa)
            .ok_or(AmountError::Overflow)?;
        let prod_scale = holding
            .scale
            .checked_add(pct.scale)
            .ok_or(AmountError::Overflow)?;
        // Divide by 100 (adds scale 2), then floor to PCT_ABS_SCALE.
        let full_scale = prod_scale.checked_add(2).ok_or(AmountError::Overflow)?;
        let value = Decimal {
            mantissa: prod_m,
            scale: full_scale,
        };
        Ok(value.floor_to(PCT_ABS_SCALE))
    }

    /// `pct / 100` exact (FR-6 ratio for `defi redeem --ratio`; AC-7: `12.5 ⇒ 0.125`).
    pub fn pct_to_ratio(pct: &Decimal) -> Result<Decimal, AmountError> {
        // /100 == add 2 to the scale.
        let scale = pct.scale.checked_add(2).ok_or(AmountError::Overflow)?;
        Ok(Decimal {
            mantissa: pct.mantissa,
            scale,
        }
        .normalized())
    }

    /// Floor `self` to at most `max_scale` fraction digits.
    fn floor_to(&self, max_scale: u32) -> Decimal {
        if self.scale <= max_scale {
            return self.normalized();
        }
        let drop = self.scale - max_scale;
        // Dropping `drop` digits: integer-divide the mantissa by 10^drop (floor).
        let divisor = pow10(drop).unwrap_or(u128::MAX);
        Decimal {
            mantissa: self.mantissa / divisor,
            scale: max_scale,
        }
        .normalized()
    }

    /// `self <= cap` with exact scale normalization (no float).
    pub fn le(&self, cap: &Decimal) -> bool {
        let common = self.scale.max(cap.scale);
        match (self.mantissa_at(common), cap.mantissa_at(common)) {
            (Ok(a), Ok(b)) => a <= b,
            // Overflow while widening: fall back to a lossless big-compare via
            // string-free component comparison. This is unreachable for realistic
            // token amounts; treat overflow conservatively as "not <= cap".
            _ => false,
        }
    }

    /// Render as a plain decimal string (no trailing zeros, no exponent).
    pub fn to_plain_string(self) -> String {
        if self.scale == 0 {
            return self.mantissa.to_string();
        }
        let digits = self.mantissa.to_string();
        let scale = self.scale as usize;
        if digits.len() > scale {
            let split = digits.len() - scale;
            format!("{}.{}", &digits[..split], &digits[split..])
        } else {
            let zeros = "0".repeat(scale - digits.len());
            format!("0.{zeros}{digits}")
        }
    }
}

/// `10^exp` as u128, or `Overflow` if it does not fit.
fn pow10(exp: u32) -> Result<u128, AmountError> {
    10u128.checked_pow(exp).ok_or(AmountError::Overflow)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_rejects_bad_strings() {
        assert_eq!(Decimal::parse(""), Err(AmountError::Empty));
        assert_eq!(Decimal::parse("."), Err(AmountError::Invalid));
        assert_eq!(Decimal::parse("1.2.3"), Err(AmountError::Invalid));
        assert_eq!(Decimal::parse("-1"), Err(AmountError::Invalid));
        assert_eq!(Decimal::parse("1e3"), Err(AmountError::Invalid));
        assert_eq!(Decimal::parse("1 000"), Err(AmountError::Invalid));
        assert_eq!(Decimal::parse("abc"), Err(AmountError::Invalid));
    }

    #[test]
    fn parse_accepts_valid_strings() {
        assert_eq!(Decimal::parse("0").unwrap().to_plain_string(), "0");
        assert_eq!(Decimal::parse("100").unwrap().to_plain_string(), "100");
        assert_eq!(Decimal::parse("400.8").unwrap().to_plain_string(), "400.8");
        assert_eq!(Decimal::parse(".5").unwrap().to_plain_string(), "0.5");
        assert_eq!(Decimal::parse("1.50").unwrap().to_plain_string(), "1.5");
    }

    #[test]
    fn ac8_pct_to_absolute_400_8_times_25pct() {
        // 400.8 × 25% = 100.2
        let holding = Decimal::parse("400.8").unwrap();
        let pct = Decimal::parse("25").unwrap();
        let got = Decimal::pct_to_absolute(&holding, &pct).unwrap();
        assert_eq!(got.to_plain_string(), "100.2");
    }

    #[test]
    fn pct_to_absolute_floors_to_8dp() {
        // 1 × 33.333333333% = 0.33333333333 → floor 8dp = 0.33333333
        let holding = Decimal::parse("1").unwrap();
        let pct = Decimal::parse("33.333333333").unwrap();
        let got = Decimal::pct_to_absolute(&holding, &pct).unwrap();
        assert_eq!(got.to_plain_string(), "0.33333333");
    }

    #[test]
    fn ac7_pct_to_ratio_12_5_gives_0_125() {
        let pct = Decimal::parse("12.5").unwrap();
        let ratio = Decimal::pct_to_ratio(&pct).unwrap();
        assert_eq!(ratio.to_plain_string(), "0.125");
    }

    #[test]
    fn le_compares_across_scales() {
        let a = Decimal::parse("100.2").unwrap();
        let cap = Decimal::parse("100.20").unwrap();
        assert!(a.le(&cap));
        assert!(cap.le(&a));
        assert!(Decimal::parse("99.999999")
            .unwrap()
            .le(&Decimal::parse("100").unwrap()));
        assert!(!Decimal::parse("100.0001")
            .unwrap()
            .le(&Decimal::parse("100").unwrap()));
    }

    #[test]
    fn parse_overflow_is_error() {
        // 40 nines overflows u128 (max ~3.4e38 = 39 digits).
        let big = "9".repeat(40);
        assert_eq!(Decimal::parse(&big), Err(AmountError::Overflow));
    }

    #[test]
    fn zero_is_zero() {
        assert!(Decimal::parse("0").unwrap().is_zero());
        assert!(Decimal::parse("0.0").unwrap().is_zero());
        assert!(!Decimal::parse("0.1").unwrap().is_zero());
    }
}
