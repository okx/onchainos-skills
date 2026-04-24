#!/usr/bin/env python3
"""
onchainos scan bot example — demonstrates scripting with the onchainos CLI.
Scans for new MIGRATED tokens on Solana, enriches top 5 with security data,
and prints a summary table.

Usage:
    python3 scan-bot-example.py

Requires: onchainos CLI installed and on PATH.
"""

import json
import subprocess
import sys

def run(cmd: list[str]) -> dict | list | None:
    """Run an onchainos command and return parsed JSON."""
    try:
        result = subprocess.run(cmd, capture_output=True, text=True, timeout=30)
        if result.returncode != 0:
            return None
        data = json.loads(result.stdout)
        return data.get("data") if isinstance(data, dict) and "data" in data else data
    except (json.JSONDecodeError, subprocess.TimeoutExpired):
        return None

def main():
    print("Scanning new MIGRATED tokens on Solana...\n")

    # Step 1: Fetch MIGRATED token list
    tokens = run(["onchainos", "memepump", "tokens", "--chain", "solana", "--stage", "MIGRATED"])
    if not tokens or not isinstance(tokens, list):
        print("No tokens found or API error.")
        sys.exit(1)

    print(f"Found {len(tokens)} tokens. Enriching top 5...\n")

    # Step 2: Enrich top 5 with security scan
    header = f"{'Symbol':<12} {'MCap':>12} {'Holders':>8} {'Honeypot':>10} {'Mint':>8}"
    print(header)
    print("-" * len(header))

    for token in tokens[:5]:
        addr = token.get("tokenContractAddress", "")
        symbol = token.get("tokenSymbol", "???")[:11]
        mcap = token.get("marketCap", "N/A")

        # Security scan
        scan = run([
            "onchainos", "security", "token-scan",
            "--tokens", f"501:{addr}"
        ])

        honeypot = "?"
        mint = "?"
        if scan and isinstance(scan, list) and scan:
            s = scan[0]
            honeypot = "YES" if s.get("isHoneypot") else "No"
            mint = "Active" if s.get("isMintable") else "Revoked"

        holders = token.get("holders", "N/A")

        print(f"{symbol:<12} {str(mcap):>12} {str(holders):>8} {honeypot:>10} {mint:>8}")

    print("\nDone.")

if __name__ == "__main__":
    main()
