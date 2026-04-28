#!/bin/bash
# onchainos — OpenClaw template build script
# Installs the onchainos CLI, skills, and workflows.
# Runs once during the build phase; no action needed from the user.

set -e

curl -sSL https://raw.githubusercontent.com/okx/onchainos-skills/main/install.sh | sh
