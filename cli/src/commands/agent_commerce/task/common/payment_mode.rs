/// Backend paymentMode: NONE(0), ESCROW(1), X402(3)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaymentMode {
    None,
    Escrow,
    X402,
}

impl PaymentMode {
    /// CLI string -> enum ("escrow" / "x402")
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
            Some("x402") => Ok(Self::X402.as_int()),
            Some(other) => anyhow::bail!("unsupported --payment-mode \"{other}\"; valid values: escrow, x402"),
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
            Self::X402 => "x402",
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
            Self::X402 => "x402 on-demand micropayment",
        }
    }
}
