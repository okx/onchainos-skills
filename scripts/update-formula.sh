#!/usr/bin/env bash
set -euo pipefail

# ──────────────────────────────────────────────────────────────
# Auto-update Homebrew Formula SHA256 checksums
#
# Fetches per-platform binary SHA256 from the GitHub Release checksums.txt,
# then rewrites Formula/onchainos.rb.
#
# Called automatically by the update-homebrew job in release.yml.
# Can also be run manually:
#   scripts/update-formula.sh v0.1.0
# ──────────────────────────────────────────────────────────────

TAG="${1:?Usage: $0 <release-tag> [repo]  (e.g. v0.1.0 okx/onchainos-skills)}"
# CI passes the actual repo name (${{ github.repository }}); defaults to okx/onchainos-skills for manual runs
REPO="${2:-okx/onchainos-skills}"
# Extract clean version number from tag (strip v prefix)
VERSION=$(echo "$TAG" | sed 's/^v//')
BIN="onchainos"
CHECKSUMS_URL="https://github.com/${REPO}/releases/download/${TAG}/checksums.txt"
FORMULA="Formula/onchainos.rb"
BASE_URL="https://github.com/${REPO}/releases/download/${TAG}"

echo "Fetching checksums for ${TAG}..."
# GitHub Release CDN may have a brief delay after creation; retry up to 5 times (10s each)
MAX_RETRIES=5
for i in $(seq 1 $MAX_RETRIES); do
  CHECKSUMS=$(curl -sSfL "$CHECKSUMS_URL" 2>/dev/null) && break
  echo "Attempt $i/$MAX_RETRIES failed, retrying in 10s..."
  sleep 10
done

if [ -z "${CHECKSUMS:-}" ]; then
  echo "ERROR: failed to fetch checksums after $MAX_RETRIES attempts" >&2
  echo "URL: $CHECKSUMS_URL" >&2
  exit 1
fi

# Extract SHA256 for a given target from checksums.txt
get_sha() {
  local target="$1"
  local sha
  sha=$(echo "$CHECKSUMS" | grep "${BIN}-${target}" | awk '{print $1}')
  if [ -z "$sha" ]; then
    echo "ERROR: no checksum found for target ${target}" >&2
    exit 1
  fi
  echo "$sha"
}

SHA_X86_MAC=$(get_sha "x86_64-apple-darwin")
SHA_ARM_MAC=$(get_sha "aarch64-apple-darwin")
SHA_X86_LINUX=$(get_sha "x86_64-unknown-linux-gnu")
SHA_ARM_LINUX=$(get_sha "aarch64-unknown-linux-gnu")

echo "Writing ${FORMULA} for version ${VERSION} (tag ${TAG})..."

cat > "$FORMULA" << RUBY
class Onchainos < Formula
  desc "onchainOS CLI — token search, market data, wallet, swap, and gateway across 20+ chains"
  homepage "https://github.com/${REPO}"
  version "${VERSION}"
  license "Apache-2.0"

  on_macos do
    on_intel do
      url "${BASE_URL}/${BIN}-x86_64-apple-darwin"
      sha256 "${SHA_X86_MAC}"
    end

    on_arm do
      url "${BASE_URL}/${BIN}-aarch64-apple-darwin"
      sha256 "${SHA_ARM_MAC}"
    end
  end

  on_linux do
    on_intel do
      url "${BASE_URL}/${BIN}-x86_64-unknown-linux-gnu"
      sha256 "${SHA_X86_LINUX}"
    end

    on_arm do
      url "${BASE_URL}/${BIN}-aarch64-unknown-linux-gnu"
      sha256 "${SHA_ARM_LINUX}"
    end
  end

  def install
    downloaded = Dir["${BIN}-*"].first || "${BIN}"
    bin.install downloaded => "${BIN}"
  end

  test do
    assert_match "${BIN}", shell_output("#{bin}/${BIN} --help")
  end
end
RUBY

echo "Done. Updated ${FORMULA} to version ${VERSION}"
echo "  x86_64-apple-darwin:       ${SHA_X86_MAC}"
echo "  aarch64-apple-darwin:      ${SHA_ARM_MAC}"
echo "  x86_64-unknown-linux-gnu:  ${SHA_X86_LINUX}"
echo "  aarch64-unknown-linux-gnu: ${SHA_ARM_LINUX}"
