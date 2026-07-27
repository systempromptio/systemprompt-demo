#!/usr/bin/env bash
# Detect unused dependencies across every workspace. Shared by `just machete`
# and the quality.yml machete CI job.
#
# The root Cargo.toml sets `exclude = ["tests", "bridge"]`, so a bare
# `cargo machete` at the root silently skips those two workspaces entirely.
# Each one has to be entered on its own.
set -euo pipefail

cd "$(dirname "$0")/.."

workspaces=". tests bridge"

for w in $workspaces; do
    # bridge/ path-depends on ../systemprompt-core/bin/bridge, which is
    # `publish = false` — without the sibling checkout the manifest cannot
    # resolve at all, so skip rather than fail. Same precondition as
    # `just bridge-build`.
    if [ "$w" = "bridge" ] && [ ! -d ../systemprompt-core ]; then
        echo "==> cargo machete: bridge (skipped — no ../systemprompt-core checkout)"
        continue
    fi
    echo "==> cargo machete: $w"
    (cd "$w" && cargo machete)
done
