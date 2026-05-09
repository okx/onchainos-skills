//! 非负十进制字符串的精确算术（加 / 减 / 比较）。
//!
//! 用途：staking 预检需要在 `activeStake` / `amount` / `minCumulativeStake` /
//! `partialUnstakeMinRetainOkb` 之间做加减比较，这些值都是后端 / 配置下发的
//! UI 字符串（OKB 单位）。直接 `f64` 减法会出现 `0.0012 - 0.0002 =
//! 0.0009999999999999998` 之类的精度抖动，把"恰好达标"误判为"差一点"。
//!
//! 实现思路：把两个操作数对齐到二者中较大的小数位数，去掉小数点后当 `u128`
//! 整数运算 —— 不依赖 token decimals，对任意精度的输入都精确。

use anyhow::{anyhow, bail, Result};
use std::cmp::Ordering;

/// 拆分非负十进制字符串为 (整数部分, 小数部分)，校验只允许数字和最多一个小数点。
fn split(s: &str) -> Result<(&str, &str)> {
    let s = s.trim();
    if s.is_empty() {
        bail!("decimal string is empty");
    }
    let (int_part, frac_part) = match s.split_once('.') {
        Some((i, f)) => (i, f),
        None => (s, ""),
    };
    let int_part = if int_part.is_empty() { "0" } else { int_part };
    if !int_part.chars().all(|c| c.is_ascii_digit()) {
        bail!("invalid decimal (non-digit in integer part): \"{s}\"");
    }
    if !frac_part.chars().all(|c| c.is_ascii_digit()) {
        bail!("invalid decimal (non-digit in fractional part): \"{s}\"");
    }
    Ok((int_part, frac_part))
}

/// 把两个十进制字符串对齐到共同精度，返回 (a 的整数表示, b 的整数表示, 共同精度)。
fn align(a: &str, b: &str) -> Result<(u128, u128, usize)> {
    let (ai, af) = split(a)?;
    let (bi, bf) = split(b)?;
    let prec = af.len().max(bf.len());
    let to_u128 = |i: &str, f: &str, original: &str| -> Result<u128> {
        // 小数部分右侧补 0 到 prec 位（width 是最小长度，不会截断）
        let combined = format!("{i}{f:0<prec$}");
        let stripped = combined.trim_start_matches('0');
        let normalized = if stripped.is_empty() { "0" } else { stripped };
        normalized
            .parse::<u128>()
            .map_err(|e| anyhow!("decimal exceeds u128 range: \"{original}\": {e}"))
    };
    Ok((to_u128(ai, af, a)?, to_u128(bi, bf, b)?, prec))
}

/// 把 u128 在指定精度下还原成规范十进制字符串：
/// - 小数部分尾部 0 截掉
/// - 整数（小数全 0）不带小数点
fn format_at(value: u128, prec: usize) -> String {
    if prec == 0 {
        return value.to_string();
    }
    let scale = 10u128.pow(prec as u32);
    let int_part = value / scale;
    let frac_part = value % scale;
    if frac_part == 0 {
        return int_part.to_string();
    }
    let frac_str = format!("{frac_part:0>prec$}");
    let trimmed = frac_str.trim_end_matches('0');
    format!("{int_part}.{trimmed}")
}

pub fn cmp(a: &str, b: &str) -> Result<Ordering> {
    let (av, bv, _) = align(a, b)?;
    Ok(av.cmp(&bv))
}

/// `a - b`，要求 `a >= b`，否则返回 underflow 错误。
pub fn sub(a: &str, b: &str) -> Result<String> {
    let (av, bv, prec) = align(a, b)?;
    let diff = av
        .checked_sub(bv)
        .ok_or_else(|| anyhow!("decimal subtraction underflow: \"{a}\" - \"{b}\""))?;
    Ok(format_at(diff, prec))
}

pub fn add(a: &str, b: &str) -> Result<String> {
    let (av, bv, prec) = align(a, b)?;
    let sum = av
        .checked_add(bv)
        .ok_or_else(|| anyhow!("decimal addition overflow: \"{a}\" + \"{b}\""))?;
    Ok(format_at(sum, prec))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sub_no_fp_artifact() {
        // 同一个 case 下 f64 与字符串十进制的对比：
        //   - f64    : 0.0012 - 0.0002 = 0.0009999999999999998（≠ 0.001）
        //   - 字符串 :                  = "0.001"               （== 0.001）
        let fp_result = 0.0012_f64 - 0.0002_f64;
        assert_ne!(fp_result, 0.001_f64);
        assert_eq!(format!("{fp_result}"), "0.0009999999999999998");

        assert_eq!(sub("0.0012", "0.0002").unwrap(), "0.001");
    }

    #[test]
    fn cmp_handles_uneven_precision() {
        // remaining=0.001 vs retain=0.001 应该相等，不能因为对齐方式被判 < 或 >
        assert_eq!(cmp("0.001", "0.0010").unwrap(), Ordering::Equal);
        assert_eq!(cmp("0.0012", "0.001").unwrap(), Ordering::Greater);
        assert_eq!(cmp("0.0009", "0.001").unwrap(), Ordering::Less);
    }

    #[test]
    fn add_simple() {
        assert_eq!(add("0.0012", "0.0008").unwrap(), "0.002");
        assert_eq!(add("1", "0.5").unwrap(), "1.5");
        assert_eq!(add("0", "0").unwrap(), "0");
    }

    #[test]
    fn integer_only_inputs() {
        assert_eq!(sub("100", "30").unwrap(), "70");
        assert_eq!(add("100", "30").unwrap(), "130");
        assert_eq!(cmp("100", "30").unwrap(), Ordering::Greater);
    }

    #[test]
    fn mixed_precision_alignment() {
        // 10.5 (1 frac) vs 0.0001 (4 frac) → 共同精度 4
        assert_eq!(sub("10.5", "0.0001").unwrap(), "10.4999");
        assert_eq!(add("10.5", "0.0001").unwrap(), "10.5001");
        assert_eq!(cmp("10.5", "0.0001").unwrap(), Ordering::Greater);
    }

    #[test]
    fn underflow_errors() {
        assert!(sub("0.001", "0.002").is_err());
    }

    #[test]
    fn invalid_inputs_error() {
        assert!(cmp("", "0").is_err());
        assert!(cmp("abc", "0").is_err());
        assert!(cmp("1.2.3", "0").is_err());
        assert!(cmp("-1", "0").is_err());
    }

    #[test]
    fn trailing_zeros_trimmed_in_output() {
        assert_eq!(sub("0.10", "0.05").unwrap(), "0.05");
        assert_eq!(add("0.5", "0.5").unwrap(), "1");
    }
}