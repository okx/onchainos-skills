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

    /// Human-readable description (Chinese display label).
    pub fn desc(&self) -> &'static str {
        match self {
            Self::None => "未设置",
            Self::Escrow => "担保支付（Escrow）",
            Self::X402 => "x402 按需微支付",
        }
    }
}
