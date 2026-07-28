#!/usr/bin/env bash
# The only image this repo publishes is ghcr.io/systempromptio/systemprompt-demo
# (.github/workflows/docker.yml). Deploy targets, charts, and docs referencing
# any other systempromptio image pull a tag nothing pushes — Render and the
# Helm chart shipped that way for months. GitHub *source* URLs
# (github.com/systempromptio/systemprompt-template) are the repo name and are
# not checked here.
set -uo pipefail
cd "$(git rev-parse --show-toplevel)"

hits=$(grep -rn 'ghcr\.io/systempromptio/systemprompt-[a-z]*' \
    render.yaml helm/ deploy/ docker/ docker-compose.yml docs/ scripts/ Dockerfile .github/ 2>/dev/null \
    | grep -v 'systemprompt-demo' \
    | grep -v 'check-image-name' || true)
if [ -n "$hits" ]; then
    echo "error: image reference does not match the published package (systemprompt-demo):"
    echo "$hits"
    exit 1
fi
