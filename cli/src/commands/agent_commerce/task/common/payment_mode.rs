/// 后端 paymentMode: NONE(0), ESCROW(1), DIRECT/NON_ESCROW(2), X402(3)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaymentMode {
    None,
    Escrow,
    NonEscrow,
    X402,
}

impl PaymentMode {
    /// CLI 字符串 → 枚举（"escrow" / "non_escrow" / "direct" / "x402"）
    pub fn from_str(s: &str) -> Self {
        match s {
            "escrow" => Self::Escrow,
            "non_escrow" | "direct" => Self::NonEscrow,
            "x402" => Self::X402,
            _ => Self::Escrow,
        }
    }

    /// 后端 int → 枚举
    pub fn from_int(i: i32) -> Self {
        match i {
            1 => Self::Escrow,
            2 => Self::NonEscrow,
            3 => Self::X402,
            _ => Self::None,
        }
    }

    /// 枚举 → CLI 字符串
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Escrow => "escrow",
            Self::NonEscrow => "non_escrow",
            Self::X402 => "x402",
        }
    }

    /// 枚举 → 后端 int
    pub fn as_int(&self) -> i32 {
        match self {
            Self::None => 0,
            Self::Escrow => 1,
            Self::NonEscrow => 2,
            Self::X402 => 3,
        }
    }

    /// 中文展示描述
    pub fn desc(&self) -> &'static str {
        match self {
            Self::None => "未设置",
            Self::Escrow => "托管支付（Escrow）",
            Self::NonEscrow => "非托管支付（Non-Escrow）",
            Self::X402 => "x402 按需支付",
        }
    }
}
