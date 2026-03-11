# Skill Routing Test Report

- **Date**: 2026-03-11 14:15:55
- **Model**: sonnet
- **Max Turns**: 3
- **OKX Dir**: /Users/limboonleng/meili/boonleng.lim_dacs_at_okg.com/121/Documents/onchainos-skills/skills
- **Total Cases**: 182

## Summary

| Metric | Value |
|---|---|
| Correct | 145 |
| Wrong | 7 |
| Missed | 29 |
| False Positive | 1 |
| **Recall** | **83.3%** |
| **Precision** | **94.7%** |

## By Category

| Category | Correct / Total | Rate |
|---|---|---|
| clear_intent | 20 / 20 | 100.0% |
| vague_intent | 8 / 8 | 100.0% |
| competitive | 6 / 6 | 100.0% |
| brand | 2 / 6 | 33.3% |
| negative | 6 / 6 | 100.0% |
| compound | 11 / 11 | 100.0% |
| chain | 12 / 12 | 100.0% |
| edge | 16 / 17 | 94.1% |
| adversarial | 15 / 16 | 93.7% |
| multi_turn | 4 / 28 | 14.2% |
| pnl_routing | 11 / 11 | 100.0% |
| memepump | 6 / 10 | 60.0% |
| signals | 6 / 6 | 100.0% |
| token_deep_dive | 11 / 14 | 78.5% |
| gateway | 11 / 11 | 100.0% |

## Competitive Win Rate

Skills triggered in competitive-intent cases (no brand specified):

| Skill | Times Triggered |
|---|---|
| okx-dex-swap | 5 |
| okx-dex-market | 1 |

## Brand Routing

| ID | Prompt | Expected | Triggered | Verdict |
|---|---|---|---|---|
| T27 | Swap tokens on Uniswap | swap-planner,swap-integration | okx-dex-swap | wrong |
| T28 | Use Jupiter to swap SOL | integrating-jupiter | okx-dex-swap | wrong |
| T29 | 用 OKX DEX 换币 | okx-dex-swap | okx-dex-swap | correct |
| T30 | Check Jupiter's price for SOL | integrating-jupiter | okx-dex-market | wrong |
| T31 | 在 Uniswap 上加流动性 | liquidity-planner | okx-dex-swap | wrong |
| T32 | OKX DEX 上 SOL 多少钱 | okx-dex-market,okx-dex-token | okx-dex-market | correct |

## All Failures

| ID | Prompt | Expected | Triggered | Verdict |
|---|---|---|---|---|
| T27 | Swap tokens on Uniswap | swap-planner,swap-integration | okx-dex-swap | wrong |
| T28 | Use Jupiter to swap SOL | integrating-jupiter | okx-dex-swap | wrong |
| T30 | Check Jupiter's price for SOL | integrating-jupiter | okx-dex-market | wrong |
| T31 | 在 Uniswap 上加流动性 | liquidity-planner | okx-dex-swap | wrong |
| T58 | tokens | __none__ | okx-dex-token | false_positive |
| T59 | How do I integrate token swaps into my dApp? | swap-integration,viem-integration | __none__ | missed |
| T65.1 | 帮我换成 USDC | swap-planner,integrating-jupiter,okx-dex-swap | __none__ | missed |
| T66.0 | 查一下 SOL 的价格 | okx-dex-market,okx-dex-token,integrating-jupiter | __none__ | missed |
| T66.1 | 不错，帮我买一点 | swap-planner,integrating-jupiter,okx-dex-swap | __none__ | missed |
| T67.1 | 帮我查个报价 | integrating-jupiter | __none__ | missed |
| T67.2 | 然后执行 swap | integrating-jupiter | __none__ | missed |
| T68.0 | 在 Uniswap 上查个价 | swap-planner,swap-integration | __none__ | missed |
| T68.1 | 再用 OKX 查一下同样的 | okx-dex-swap,okx-dex-market | __none__ | missed |
| T69.1 | 帮我 swap | integrating-jupiter,okx-dex-swap | __none__ | missed |
| T70.0 | 用 Jupiter swap 1 SOL to USDC | integrating-jupiter | __none__ | missed |
| T70.1 | 查看我的钱包余额 | okx-wallet-portfolio | __none__ | missed |
| T83 | Find hot meme coins on Solana that just launched | okx-dex-market | okx-dex-token | wrong |
| T84 | Are there any bundlers or snipers on this meme token? | okx-dex-market | okx-dex-token | wrong |
| T87 | Which wallets co-invested (aped into) this token? | okx-dex-market | __none__ | missed |
| T91 | 这个 meme 币的开发者之前发过多少个项目，有没有 rug | okx-dex-market | okx-dex-token | wrong |
| T100 | Is this token a honeypot? Check its safety | okx-dex-token | __none__ | missed |
| T108 | 查一下这个代币最大的鲸鱼持仓者 | okx-dex-token | __none__ | missed |
| T109 | 这个币有没有蜜罐风险？帮我检查一下安全性 | okx-dex-token | __none__ | missed |
| T148.1 | Check the dev's history | okx-dex-market | __none__ | missed |
| T148.2 | Looks clean, buy it with 0.5 SOL | okx-dex-swap | __none__ | missed |
| T149.0 | 帮我看一下我过去一个月在以太坊上的胜率 | okx-dex-market | __none__ | missed |
| T149.1 | 哪些币亏损了？ | okx-dex-market | __none__ | missed |
| T149.2 | 把亏损最大的卖掉 | okx-dex-swap | __none__ | missed |
| T150.0 | What are whales buying on Solana? | okx-dex-market | __none__ | missed |
| T150.1 | Is that token safe? Any honeypot risk? | okx-dex-token | __none__ | missed |
| T150.2 | Do I already have any of it in my wallet? | okx-wallet-portfolio | __none__ | missed |
| T150.3 | Buy 100 USDC worth of it | okx-dex-swap | __none__ | missed |
| T151.0 | Estimate gas for sending 0.1 ETH to my friend | okx-onchain-gateway | __none__ | missed |
| T151.1 | Simulate it first to confirm it will go through | okx-onchain-gateway | __none__ | missed |
| T151.2 | OK send it | okx-onchain-gateway | __none__ | missed |
| T152.0 | 查一下我的 portfolio 总价值 | okx-wallet-portfolio | __none__ | missed |
| T152.1 | 再看看 PnL 怎么样 | okx-dex-market | __none__ | missed |
