/// Backend paymentMode: NONE(0), ESCROW(1), legacy-disabled X402(3).
///
/// The X402 variant is retained only so stale backend records fail closed. New
/// Task commands can create or select escrow mode only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaymentMode {
    None,
    Escrow,
    X402,
}

impl PaymentMode {
    /// CLI string -> enum. `x402` is recognized only as a disabled sentinel.
    pub fn from_str(s: &str) -> Self {
        match s {
            "escrow" => Self::Escrow,
            "x402" => Self::X402,
            _ => Self::Escrow,
        }
    }

    /// Parse an optional CLI `--payment-mode` flag into its backend int.
    /// Returns 0 (unset) when `flag` is `None`; errors on unknown values.
    pub fn parse_flag(flag: Option<&str>) -> anyhow::Result<i32> {
        match flag {
            None => Ok(0),
            Some("escrow") => Ok(Self::Escrow.as_int()),
            Some(other) => {
                anyhow::bail!("unsupported --payment-mode \"{other}\"; valid Task value: escrow")
            }
        }
    }

    /// Backend int -> enum
    pub fn from_int(i: i32) -> Self {
        match i {
            1 => Self::Escrow,
            3 => Self::X402,
            _ => Self::None,
        }
    }

    /// Enum -> CLI string
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Escrow => "escrow",
            Self::X402 => "legacy-x402-disabled",
        }
    }

    /// Enum -> backend int
    pub fn as_int(&self) -> i32 {
        match self {
            Self::None => 0,
            Self::Escrow => 1,
            Self::X402 => 3,
        }
    }

    /// Human-readable description.
    pub fn desc(&self) -> &'static str {
        match self {
            Self::None => "not set",
            Self::Escrow => "escrow payment",
            Self::X402 => "legacy task payment disabled",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::PaymentMode;

    #[test]
    fn new_task_flags_accept_escrow_and_reject_legacy_x402() {
        assert_eq!(PaymentMode::parse_flag(Some("escrow")).unwrap(), 1);
        assert!(PaymentMode::parse_flag(Some("x402")).is_err());
    }
}
