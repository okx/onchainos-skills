/// Shared enum resolvers that map human-readable names to API integer codes.
///
/// Each resolver accepts either a name (case-insensitive) or the raw integer string,
/// returning the integer string expected by the API. Unknown values pass through unchanged.

/// Resolve signal wallet type: smart_money/kol/whale → 1/2/3
/// Used by: `signal list --wallet-type`
/// Note: comma-separated values are supported (e.g. "smart_money,whale" → "1,3")
pub fn resolve_signal_wallet_type(input: &str) -> String {
    input
        .split(',')
        .map(|s| match s.trim().to_lowercase().as_str() {
            "smart_money" | "smartmoney" => "1",
            "kol" | "influencer" => "2",
            "whale" | "whales" => "3",
            other => other,
        })
        .collect::<Vec<_>>()
        .join(",")
}

/// Resolve token tag filter: kol/developer/smart_money/... → 1-9
/// Used by: `token holders --tag-filter`, `token top-trader --tag-filter`, `token trades --tag-filter`
pub fn resolve_tag_filter(input: &str) -> String {
    match input.to_lowercase().as_str() {
        "kol" | "influencer" => "1".to_string(),
        "developer" | "dev" => "2".to_string(),
        "smart_money" | "smartmoney" => "3".to_string(),
        "whale" | "whales" => "4".to_string(),
        "fresh_wallet" | "fresh" => "5".to_string(),
        "insider" => "6".to_string(),
        "sniper" => "7".to_string(),
        "suspicious" | "phishing" => "8".to_string(),
        "bundler" | "bundle" => "9".to_string(),
        other => other.to_string(),
    }
}

/// Resolve leaderboard time frame: 1d/3d/7d/1m/3m → 1-5
/// Used by: `leaderboard list --time-frame`
pub fn resolve_leaderboard_time_frame(input: &str) -> String {
    match input.to_lowercase().as_str() {
        "1d" | "today" => "1".to_string(),
        "3d" => "2".to_string(),
        "7d" | "1w" => "3".to_string(),
        "1m" | "30d" => "4".to_string(),
        "3m" | "90d" => "5".to_string(),
        other => other.to_string(),
    }
}

/// Resolve leaderboard sort field: pnl/win_rate/txs/volume/roi → 1-5
/// Used by: `leaderboard list --sort-by`
pub fn resolve_leaderboard_sort_by(input: &str) -> String {
    match input.to_lowercase().as_str() {
        "pnl" => "1".to_string(),
        "win_rate" | "winrate" => "2".to_string(),
        "txs" | "tx_count" => "3".to_string(),
        "volume" => "4".to_string(),
        "roi" => "5".to_string(),
        other => other.to_string(),
    }
}

/// Resolve market portfolio time frame: 1d/3d/7d/1m/3m → 1-5
/// Used by: `market portfolio-overview --time-frame`
pub fn resolve_market_time_frame(input: &str) -> String {
    match input.to_lowercase().as_str() {
        "1d" | "today" => "1".to_string(),
        "3d" => "2".to_string(),
        "7d" | "1w" => "3".to_string(),
        "1m" | "30d" => "4".to_string(),
        "3m" | "90d" => "5".to_string(),
        other => other.to_string(),
    }
}

/// Resolve hot-tokens ranking type: trending/xmentioned → 4/5
/// Used by: `token hot-tokens --ranking-type`
pub fn resolve_ranking_type(input: &str) -> String {
    match input.to_lowercase().as_str() {
        "trending" | "score" => "4".to_string(),
        "xmentioned" | "x" | "twitter" => "5".to_string(),
        other => other.to_string(),
    }
}

/// Resolve hot-tokens time frame: 5m/1h/4h/24h → 1-4
/// Used by: `token hot-tokens --time-frame`
pub fn resolve_hot_tokens_time_frame(input: &str) -> String {
    match input.to_lowercase().as_str() {
        "5m" | "5min" => "1".to_string(),
        "1h" => "2".to_string(),
        "4h" => "3".to_string(),
        "24h" | "1d" => "4".to_string(),
        other => other.to_string(),
    }
}

/// Resolve cluster top-holders range: top10/top50/top100 → 1/2/3
/// Used by: `token cluster-top-holders --range-filter`
pub fn resolve_range_filter(input: &str) -> String {
    match input.to_lowercase().as_str() {
        "top10" | "top_10" => "1".to_string(),
        "top50" | "top_50" => "2".to_string(),
        "top100" | "top_100" => "3".to_string(),
        other => other.to_string(),
    }
}

/// Resolve tracker trade type: all/buy/sell → 0/1/2
/// Used by: `tracker activities --trade-type`
pub fn resolve_trade_type(input: &str) -> String {
    match input.to_lowercase().as_str() {
        "all" => "0".to_string(),
        "buy" => "1".to_string(),
        "sell" => "2".to_string(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signal_wallet_type_names() {
        assert_eq!(resolve_signal_wallet_type("smart_money"), "1");
        assert_eq!(resolve_signal_wallet_type("kol"), "2");
        assert_eq!(resolve_signal_wallet_type("whale"), "3");
        assert_eq!(resolve_signal_wallet_type("smart_money,whale"), "1,3");
        assert_eq!(resolve_signal_wallet_type("1,3"), "1,3");
    }

    #[test]
    fn tag_filter_names() {
        assert_eq!(resolve_tag_filter("kol"), "1");
        assert_eq!(resolve_tag_filter("smart_money"), "3");
        assert_eq!(resolve_tag_filter("whale"), "4");
        assert_eq!(resolve_tag_filter("sniper"), "7");
        assert_eq!(resolve_tag_filter("bundler"), "9");
        assert_eq!(resolve_tag_filter("3"), "3");
    }

    #[test]
    fn leaderboard_time_frame_names() {
        assert_eq!(resolve_leaderboard_time_frame("1d"), "1");
        assert_eq!(resolve_leaderboard_time_frame("7d"), "3");
        assert_eq!(resolve_leaderboard_time_frame("3m"), "5");
        assert_eq!(resolve_leaderboard_time_frame("2"), "2");
    }

    #[test]
    fn leaderboard_sort_by_names() {
        assert_eq!(resolve_leaderboard_sort_by("pnl"), "1");
        assert_eq!(resolve_leaderboard_sort_by("win_rate"), "2");
        assert_eq!(resolve_leaderboard_sort_by("roi"), "5");
    }

    #[test]
    fn ranking_type_names() {
        assert_eq!(resolve_ranking_type("trending"), "4");
        assert_eq!(resolve_ranking_type("xmentioned"), "5");
        assert_eq!(resolve_ranking_type("4"), "4");
    }

    #[test]
    fn hot_tokens_time_frame_names() {
        assert_eq!(resolve_hot_tokens_time_frame("5m"), "1");
        assert_eq!(resolve_hot_tokens_time_frame("1h"), "2");
        assert_eq!(resolve_hot_tokens_time_frame("24h"), "4");
    }

    #[test]
    fn range_filter_names() {
        assert_eq!(resolve_range_filter("top10"), "1");
        assert_eq!(resolve_range_filter("top100"), "3");
        assert_eq!(resolve_range_filter("2"), "2");
    }

    #[test]
    fn trade_type_names() {
        assert_eq!(resolve_trade_type("buy"), "1");
        assert_eq!(resolve_trade_type("sell"), "2");
        assert_eq!(resolve_trade_type("all"), "0");
    }
}
