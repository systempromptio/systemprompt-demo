#!/usr/bin/env bash
# No function body in an extension source may exceed 75 lines. Shared by
# `just function-size` and the quality.yml source-gates job.
#
# The file ceiling in check-file-size.sh is a cohesion proxy for a *module*; it
# says nothing about one function, so a 150-line function sat inside a passing
# file. The limit counts the body — signature and closing brace excluded — so
# splitting a parameter list across lines never costs anything.
#
# Exemption: a function that genuinely cannot be split (a long `match` over an
# external vocabulary, a generated table) may annotate its `fn` line with
# `// lint-ok: function-size <reason>`.
set -euo pipefail

cd "$(dirname "$0")/.."

LIMIT=75

violations=$(find extensions -name '*.rs' \
    -not -path '*/target/*' \
    -not -path '*/tests/*' \
    -exec awk -v limit="$LIMIT" '
        FNR == 1 { depth = 0; in_fn = 0 }
        {
            line = $0
            # Strip line comments and string bodies so their braces do not count.
            gsub(/"([^"\\]|\\.)*"/, "\"\"", line)
            sub(/\/\/.*$/, "", line)

            if (!in_fn && line ~ /(^|[^A-Za-z_])fn[[:space:]]+[A-Za-z_]/) {
                if ($0 ~ /lint-ok: function-size/) { skip = 1 } else { skip = 0 }
                pending = 1
                start = FNR
                name = $0
                sub(/^[[:space:]]*/, "", name)
            }

            if (pending || in_fn) {
                n = gsub(/\{/, "{", line)
                m = gsub(/\}/, "}", line)
                if (pending && n > 0) { pending = 0; in_fn = 1; depth = 0; open = FNR }
                depth += n - m
                if (in_fn && depth <= 0) {
                    body = FNR - open - 1
                    if (body > limit && !skip)
                        printf "%d\t%s:%d\t%s\n", body, FILENAME, start, substr(name, 1, 70)
                    in_fn = 0
                }
            }
        }
    ' {} + | sort -rn)

if [ -n "$violations" ]; then
    echo "error: function body/bodies exceed the ${LIMIT}-line ceiling:"
    printf '%s\n' "$violations"
    echo
    echo "Extract a helper, or annotate the fn line with '// lint-ok: function-size <reason>'."
    exit 1
fi
echo "All extension functions within the ${LIMIT}-line ceiling."
