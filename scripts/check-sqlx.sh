#!/usr/bin/env bash
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

# Match sqlx::query( and sqlx::query_{as,scalar,file,file_as,file_scalar,with,...}(
pattern='sqlx::query[a-z_]*\('

allowlist=(
    # Test crates run live against a freshly-migrated DB with no `.sqlx`
    # offline cache, so the compile-time macros are unavailable there.
    '^tests/'
)

allowlist_re=$(IFS='|'; echo "${allowlist[*]}")

# Drop lines that match the verified macro form (query!(), query_as!(), etc).
# Without the fallback a missing rg would empty `hits` and pass the gate
# silently — the grep branch keeps the check honest on machines without it.
if command -v rg >/dev/null 2>&1; then
    raw=$(rg -n "$pattern" extensions/ src/ --glob '*.rs' || true)
else
    raw=$(grep -rEn --include='*.rs' "$pattern" extensions/ src/ || true)
fi
hits=$(
    { echo "$raw" \
        | grep -Ev "^(${allowlist_re})" \
        | grep -Ev 'sqlx::query[a-z_]*!' | grep -v '^$' || true; }
)

if [[ -n "${hits}" ]]; then
    echo "❌ Unverified sqlx::query calls found outside the allowlist:" >&2
    echo "${hits}" >&2
    echo "" >&2
    echo "Use sqlx::query!() / query_as!() / query_scalar!() (compile-time verified)." >&2
    echo "If the call must stay dynamic, add the path to scripts/check-sqlx.sh allowlist with justification." >&2
    exit 1
fi

echo "✅ No unverified sqlx::query calls outside the allowlist."
