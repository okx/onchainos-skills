#!/bin/bash
# onchainos — OpenClaw template build script
# Installs the onchainos CLI, skills, and workflows.
# Runs once during the build phase; no action needed from the user.

set -e

curl -sSL https://raw.githubusercontent.com/okx/onchainos-skills/main/install.sh | sh

# Ensure onchainos is on PATH for the current session
# (install.sh adds it to .profile but that only applies to new terminals)
[ -f "$HOME/.profile" ] && source "$HOME/.profile"
