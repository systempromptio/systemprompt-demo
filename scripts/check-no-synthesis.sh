#!/usr/bin/env bash
# No synthesized principals: UserId::new with a string literal is forbidden in
# production code. UserId::new must take a validated identifier (from cookie,
# query, JWT claim, or DB row), never a hard-coded literal. Legitimate
# bootstrap code belongs under extensions/**/bootstrap/. Test code annotates
# the offending line with `// lint-ok: no-synthesis <reason>`, matching
# check-http-errors.sh and check-test-value.sh — the marker must sit on the
# line it excuses so it shows up in review next to the exemption.
#
# Shared by `just lint-no-synthesis` and the quality workflow so the local and
# CI gates cannot drift.
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

hits=$(grep -rEn 'UserId::new\("' extensions/ \
    --include='*.rs' \
    --exclude-dir=tests \
    --exclude-dir=bootstrap \
    | grep -v 'lint-ok: no-synthesis' \
    || true)
if [ -n "$hits" ]; then
    echo "error: forbidden synthesized principal — UserId::new with string literal"
    echo "$hits"
    echo
    echo "UserId::new must take a validated identifier (from cookie, query,"
    echo "JWT claim, or DB row), never a hard-coded literal. If this is"
    echo "legitimate bootstrap code, move it to extensions/**/bootstrap/."
    echo "Test code may annotate the line with '// lint-ok: no-synthesis <reason>'."
    exit 1
fi
