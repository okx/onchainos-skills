#!/usr/bin/env python3
"""
Copy Smart Money Bot
====================
Subscribes to onchainos `kol_smartmoney-tracker-activity` WebSocket via the
CLI, filters smart-money buys, runs a security token-scan gate, and auto-swaps
the configured funding token (e.g. USDC) into the same token at a fixed USD
size.

Pipeline per event:
    poll -> dedupe(--since cursor) -> filter(tag, size, chain, quote-token blacklist)
        -> security token-scan (skip if riskLevel exceeds max_risk_level)
        -> daily-budget + per-token-cooldown + per-token-per-day cap
        -> swap execute (or log if dry_run)

Usage:
    onchainos wallet login                     # one-time, per chain
    cp config.example.json config.json
    # edit wallets / chains / size / limits
    python bot.py --config config.json
    python bot.py --config config.json --dry-run     # log only, no broadcast

Stop with Ctrl-C; the bot stops the underlying `onchainos ws` session cleanly.
"""

import argparse
import json
import logging
import os
import shlex
import signal
import subprocess
import sys
import threading
import time
from datetime import date, datetime
from pathlib import Path
from typing import Any

RISK_ORDER = {"LOW": 0, "MEDIUM": 1, "HIGH": 2, "CRITICAL": 3}
ONCHAINOS = os.environ.get("ONCHAINOS_BIN", "onchainos")


def run_cli(args: list[str], timeout: int = 60) -> dict[str, Any]:
    cmd = [ONCHAINOS, *args, "--format", "json"]
    logging.debug("$ %s", " ".join(shlex.quote(a) for a in cmd))
    proc = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout)
    if proc.returncode != 0:
        raise RuntimeError(
            f"CLI failed ({proc.returncode}) for `{' '.join(args)}`: "
            f"{proc.stderr.strip() or proc.stdout.strip()}"
        )
    out = proc.stdout.strip()
    if not out:
        return {}
    try:
        parsed = json.loads(out)
    except json.JSONDecodeError as exc:
        raise RuntimeError(f"non-JSON CLI output: {out[:200]}") from exc
    return parsed.get("data", parsed) if isinstance(parsed, dict) else parsed


def load_config(path: Path) -> dict[str, Any]:
    with path.open() as fh:
        return json.load(fh)


def fresh_state() -> dict[str, Any]:
    return {
        "date": str(date.today()),
        "spent_usd": 0.0,
        "last_buy_ts": {},
        "buy_count": {},
    }


def load_state(path: Path) -> dict[str, Any]:
    if not path.exists():
        return fresh_state()
    with path.open() as fh:
        st = json.load(fh)
    if st.get("date") != str(date.today()):
        return fresh_state()
    return st


def save_state(path: Path, state: dict[str, Any]) -> None:
    tmp = path.with_suffix(".tmp")
    with tmp.open("w") as fh:
        json.dump(state, fh, indent=2)
    tmp.replace(path)


class Bot:
    def __init__(self, cfg: dict[str, Any], state_path: Path):
        self.cfg = cfg
        self.state_path = state_path
        self.state = load_state(state_path)
        self.state_lock = threading.Lock()
        self.session_id: str | None = None
        self.stop_flag = threading.Event()
        self.last_since_ms = int(time.time() * 1000)

    # ── lifecycle ────────────────────────────────────────────────────────────
    def start_ws(self) -> str:
        result = run_cli(
            ["ws", "start", "--channel", "kol_smartmoney-tracker-activity"]
        )
        sess = result.get("id")
        if not sess:
            raise RuntimeError(f"could not determine session id from: {result}")
        logging.info("ws session %s (status=%s)", sess, result.get("status"))
        return sess

    def stop_ws(self) -> None:
        if not self.session_id:
            return
        try:
            run_cli(["ws", "stop", "--id", self.session_id])
            logging.info("ws session stopped: %s", self.session_id)
        except Exception as exc:
            logging.warning("ws stop failed: %s", exc)

    # ── core loop ────────────────────────────────────────────────────────────
    def run(self) -> None:
        self.session_id = self.start_ws()
        interval = float(self.cfg.get("poll_interval_seconds", 5))
        try:
            while not self.stop_flag.is_set():
                self.poll_once()
                self.stop_flag.wait(interval)
        finally:
            self.stop_ws()

    def poll_once(self) -> None:
        args = [
            "ws", "poll",
            "--id", self.session_id,
            "--channel", "kol_smartmoney-tracker-activity",
            "--tag", self.cfg["filters"].get("tag", "smart_money"),
            "--trade-type", "buy",
            "--since", str(self.last_since_ms),
            "--limit", "100",
        ]
        min_q = self.cfg["filters"].get("min_quote_amount_usd")
        if min_q is not None:
            args += ["--min-quote-amount", str(min_q)]
        try:
            result = run_cli(args, timeout=30)
        except Exception as exc:
            logging.warning("poll failed: %s", exc)
            return

        status = result.get("daemon_status", "")
        if status.startswith("disconnected"):
            logging.warning("daemon %s — events may be stale until reconnect", status)

        trades = result.get("trades") or []
        if not trades:
            return
        logging.info("poll: %d new trade(s)", len(trades))

        max_ts = self.last_since_ms
        for trade in trades:
            try:
                ts = int(trade.get("tradeTime", "0") or 0)
                if ts > max_ts:
                    max_ts = ts
                self.handle_trade(trade)
            except Exception as exc:
                logging.exception("handle_trade error: %s", exc)
        self.last_since_ms = max_ts + 1

    # ── per-trade pipeline ───────────────────────────────────────────────────
    def handle_trade(self, trade: dict[str, Any]) -> None:
        chain_idx = str(trade.get("chainIndex", ""))
        chain = self.cfg["chain_index_map"].get(chain_idx)
        if not chain:
            logging.debug("skip: unmapped chainIndex=%s", chain_idx)
            return

        wallet = self.cfg["wallets"].get(chain)
        if not wallet:
            logging.debug("skip: no wallet configured for chain=%s", chain)
            return

        token_addr_raw = trade.get("tokenContractAddress") or ""
        token_addr = (
            token_addr_raw.lower() if chain != "solana" else token_addr_raw
        )
        symbol = trade.get("tokenSymbol") or "?"
        if not token_addr:
            return

        skip_quotes = {q.upper() for q in self.cfg["filters"].get("skip_quote_tokens", [])}
        quote_sym = (trade.get("quoteTokenSymbol") or "").upper()
        if quote_sym in skip_quotes:
            logging.info("skip %s: quote token %s on blacklist", symbol, quote_sym)
            return

        max_risk = self.cfg["filters"].get("max_risk_level", "MEDIUM").upper()
        risk_level = self.scan_token(token_addr, chain)
        if risk_level is None:
            logging.warning("skip %s: token-scan unavailable (fail-safe)", symbol)
            return
        if RISK_ORDER.get(risk_level, 99) > RISK_ORDER.get(max_risk, 1):
            logging.info(
                "skip %s on %s: riskLevel=%s exceeds max=%s",
                symbol, chain, risk_level, max_risk,
            )
            return

        size_usd = float(self.cfg["trade_size_usd"])
        key = f"{chain}:{token_addr}"
        with self.state_lock:
            day_budget = float(self.cfg["limits"].get("daily_budget_usd", 0))
            if day_budget and self.state["spent_usd"] + size_usd > day_budget:
                logging.info(
                    "skip %s: daily budget would exceed (%.2f + %.2f > %.2f)",
                    symbol, self.state["spent_usd"], size_usd, day_budget,
                )
                return

            cooldown = int(self.cfg["limits"].get("cooldown_seconds_per_token", 0))
            now = time.time()
            if cooldown and now - self.state["last_buy_ts"].get(key, 0) < cooldown:
                logging.info("skip %s: token cooldown active", symbol)
                return

            max_per_day = int(self.cfg["limits"].get("max_per_token_per_day", 0))
            if max_per_day and self.state["buy_count"].get(key, 0) >= max_per_day:
                logging.info("skip %s: per-token-per-day cap reached", symbol)
                return

        logging.info(
            "BUY %s on %s (riskLevel=%s, smartMoney=%s, quoteAmt=%s %s, signaler=%s)",
            symbol, chain, risk_level,
            trade.get("trackerType"),
            trade.get("quoteTokenAmount"), trade.get("quoteTokenSymbol"),
            trade.get("walletAddress"),
        )

        if self.cfg.get("dry_run", False):
            logging.info(
                "[dry-run] would swap %.4f %s -> %s on %s via %s",
                size_usd, self.cfg["funding_token"], symbol, chain, wallet,
            )
            self._record_buy(key, size_usd)
            return

        try:
            tx = self.execute_swap(chain, token_addr, wallet, size_usd)
            logging.info(
                "SWAP broadcast: %s tx=%s priceImpact=%s",
                symbol, tx.get("swapTxHash"), tx.get("priceImpact"),
            )
            self._record_buy(key, size_usd)
        except Exception as exc:
            logging.error("swap failed for %s on %s: %s", symbol, chain, exc)

    def scan_token(self, addr: str, chain: str) -> str | None:
        try:
            res = run_cli(
                ["security", "token-scan", "--token-address", addr, "--chain", chain],
                timeout=30,
            )
            level = (res.get("riskLevel") or "").upper()
            return level if level in RISK_ORDER else None
        except Exception as exc:
            logging.warning("token-scan error for %s on %s: %s", addr, chain, exc)
            return None

    def execute_swap(
        self, chain: str, token_addr: str, wallet: str, size_usd: float
    ) -> dict[str, Any]:
        return run_cli(
            [
                "swap", "execute",
                "--from", self.cfg["funding_token"],
                "--to", token_addr,
                "--readable-amount", str(size_usd),
                "--chain", chain,
                "--wallet", wallet,
                "--gas-level", "fast",
            ],
            timeout=180,
        )

    def _record_buy(self, key: str, size_usd: float) -> None:
        with self.state_lock:
            self.state["spent_usd"] += size_usd
            self.state["last_buy_ts"][key] = time.time()
            self.state["buy_count"][key] = self.state["buy_count"].get(key, 0) + 1
            save_state(self.state_path, self.state)


def main() -> int:
    ap = argparse.ArgumentParser(description="Copy Smart Money Bot (onchainos)")
    ap.add_argument("--config", required=True, type=Path)
    ap.add_argument(
        "--dry-run",
        action="store_true",
        help="log intended trades without broadcasting (overrides config)",
    )
    args = ap.parse_args()

    cfg = load_config(args.config)
    if args.dry_run:
        cfg["dry_run"] = True

    log_file = cfg.get("log_file", "bot.log")
    logging.basicConfig(
        level=logging.INFO,
        format="%(asctime)s [%(levelname)s] %(message)s",
        handlers=[
            logging.FileHandler(log_file),
            logging.StreamHandler(sys.stderr),
        ],
    )
    logging.info("started %s (dry_run=%s)", datetime.now().isoformat(timespec="seconds"), cfg.get("dry_run", False))

    bot = Bot(cfg, Path(cfg.get("state_file", "state.json")))

    def handle_signal(_sig, _frame):
        logging.info("shutdown signal received")
        bot.stop_flag.set()

    signal.signal(signal.SIGINT, handle_signal)
    signal.signal(signal.SIGTERM, handle_signal)

    try:
        bot.run()
    except Exception as exc:
        logging.exception("fatal: %s", exc)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
