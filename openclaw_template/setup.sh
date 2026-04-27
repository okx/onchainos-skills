#!/bin/bash
# onchainos — OpenClaw template build script
# Installs the onchainos CLI + workflows (via install.sh)
# Skills are installed separately by the agent — see .openclaw/INSTALL.md

set -e

curl -sSL https://raw.githubusercontent.com/okx/onchainos-skills/main/install.sh | sh

# Ensure onchainos is on PATH for the current session
[ -f "$HOME/.profile" ] && source "$HOME/.profile"
