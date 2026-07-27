#!/usr/bin/env bash
# Regenerate the Claude Code plugin bundle under storage/files/plugins/ from
# services/skills/.
#
# The bundle is the delivery artifact for `claude plugin marketplace add` and
# for the bridge's Claude Desktop / Cowork targets. Its skills are the same
# skills, in the Agent Skills on-disk shape: a kebab-case directory holding a
# SKILL.md whose frontmatter carries `name` and `description`.
#
# Before this script those bodies were maintained by hand in two places inside
# the bundle plus once in services/skills/, and they had already drifted. There
# is one source now; run this after changing a skill, and commit the result.
#
# The two bundle trees are not a mistake, though they look like one:
#   storage/files/plugins/.claude-plugin/marketplace.json
#       -> treats systemprompt/ as a PLUGIN  (so skills live at systemprompt/skills/)
#   storage/files/plugins/systemprompt/.claude-plugin/marketplace.json
#       -> treats systemprompt/ as a MARKETPLACE whose plugin is plugins/systemprompt/
#          (so skills live at systemprompt/plugins/systemprompt/skills/)
# Both are published, so both are written, from the same source.
#
# macOS/Linux safe: no GNU-only flags, no sed -i, no sha256sum.

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
src="$repo_root/services/skills"
bundle="$repo_root/storage/files/plugins/systemprompt"

if [ ! -d "$src" ]; then
  echo "generate-plugin-bundle: no services/skills at $src" >&2
  exit 1
fi

python3 - "$src" "$bundle" <<'PY'
import pathlib
import sys

src = pathlib.Path(sys.argv[1])
bundle = pathlib.Path(sys.argv[2])
targets = [bundle / "skills", bundle / "plugins" / "systemprompt" / "skills"]


def scalar(path, key):
    """Read one top-level scalar out of a flat skill config.yaml.

    Deliberately not a YAML parse: these files are flat by construction (the
    loader rejects unknown keys), and a dependency on PyYAML would make a
    bundle regeneration fail on a host that can build the whole workspace.
    """
    for line in path.read_text(encoding="utf-8").splitlines():
        if not line.startswith(f"{key}:"):
            continue
        value = line[len(key) + 1 :].strip()
        if len(value) >= 2 and value[0] == value[-1] and value[0] in "\"'":
            value = value[1:-1]
        return value
    raise SystemExit(f"{path}: no top-level `{key}:`")


def emit(skill_dir):
    config = skill_dir / "config.yaml"
    body_file = skill_dir / scalar(config, "file")
    skill_id = scalar(config, "id")
    description = scalar(config, "description")

    # pi validates `name` against a-z0-9- and rejects anything else, and Claude
    # Code is happy with the same. The human-readable name lives in the body's
    # H1, so nothing is lost by using the id here.
    name = skill_id.replace("_", "-")
    if '"' in description:
        raise SystemExit(f"{config}: description contains a quote; frontmatter would break")

    front = f'---\nname: "{name}"\ndescription: "{description}"\n---\n\n'
    body = body_file.read_text(encoding="utf-8")

    for target in targets:
        out = target / name
        out.mkdir(parents=True, exist_ok=True)
        (out / "SKILL.md").write_text(front + body, encoding="utf-8")
    return name


live = set()
for skill_dir in sorted(src.iterdir()):
    if (skill_dir / "config.yaml").exists():
        live.add(emit(skill_dir))

if not live:
    raise SystemExit("no skills found; refusing to empty the bundle")

# Remove skills the source no longer has, so a deletion propagates instead of
# leaving a stale skill in the published plugin.
removed = []
for target in targets:
    if not target.is_dir():
        continue
    for existing in sorted(target.iterdir()):
        if existing.is_dir() and existing.name not in live:
            for path in sorted(existing.rglob("*"), reverse=True):
                path.rmdir() if path.is_dir() else path.unlink()
            existing.rmdir()
            removed.append(str(existing.relative_to(bundle.parent)))

print(f"skills: {', '.join(sorted(live))}")
for path in removed:
    print(f"removed: {path}")
PY
