#!/usr/bin/env bash
set -uo pipefail

# Machete rule ported from systemprompt-core: inline `//` comments are banned
# in production sources.
#
# The only permitted full-line inline comments are the two whitelisted
# justification prefixes mandated by the rust-coding-standards skill:
#
#   // Why:  — a non-obvious invariant, hidden constraint, or exemption
#              justification (e.g. a permitted `let _ =`)
#   // JSON: — a sanctioned `serde_json::Value` protocol-boundary usage
#
# Two fork-local functional markers are also allowed — they are exemption
# annotations consumed by other gate scripts, not prose comments:
#
#   // lint-ok: <rule> — consumed by check-dead-repository-code.sh
#   // doc-ok: <reason> — consumed by check-comments.sh
#
# Continuation lines of a whitelisted comment block are allowed. `//!` module
# heads are governed separately (rustdoc placement rules), as are `///` docs
# on public API items. Test sources, `build.rs`, and the standalone `bridge/`
# workspace are out of scope.
#
# A second check flags `///` rustdoc on items that are NOT public API —
# `pub(crate)` and `pub(super)` items (rustdoc is never rendered for them).
# A genuine invariant on such an item belongs in a `// Why:` comment.

MATCHES=""
while IFS= read -r file; do
    case "$file" in
        tests/*|*/tests/*) continue ;;
        */build.rs) continue ;;
        bridge/*) continue ;;
    esac
    [ -f "$file" ] || continue
    FOUND=$(awk '
        /^[[:space:]]*\/\/\// { prev_allowed = 0; if (!in_doc) doc_line = FNR; in_doc = 1; next }
        /^[[:space:]]*\/\/!/ { prev_allowed = 0; next }
        /^[[:space:]]*\/\// {
            in_doc = 0
            if ($0 ~ /^[[:space:]]*\/\/ (Why|JSON|lint-ok|doc-ok):/) { prev_allowed = 1; next }
            if (prev_allowed) { next }
            print FILENAME ":" FNR ":" $0
            next
        }
        /^[[:space:]]*#!?\[/ { next }
        {
            if (in_doc) {
                stripped = $0
                sub(/^[[:space:]]+/, "", stripped)
                if (stripped ~ /^(pub\(crate\)|pub\(super\)|pub\(in )/ ||
                    stripped ~ /^(async fn|fn|struct|enum|const|static|type|impl|mod|trait) /) {
                    print FILENAME ":" doc_line ": rustdoc on non-public item (" stripped ") — use // Why: or delete"
                }
            }
            in_doc = 0
            prev_allowed = 0
        }
    ' "$file")
    [ -n "$FOUND" ] && MATCHES+="${FOUND}"$'\n'
done < <(git ls-files 'extensions/**/*.rs' 'src/*.rs' 'src/**/*.rs' | sort -u)

if [ -z "$MATCHES" ]; then
    echo "lint-inline-comments: OK (no unlisted inline comments)"
    exit 0
fi

echo "lint-inline-comments: inline // comments are banned in production sources."
echo "Delete the comment, or justify it with a '// Why:' or '// JSON:' prefix:"
echo ""
printf '%s' "$MATCHES"
exit 1
