#!/usr/bin/env bash
# Front-end integrity gate: JavaScript and CSS under storage/files/, the Rust
# asset registrations that ship them, and the templates that link them.
#
# The failures this catches all reached production before it existed:
#   - two classic scripts both declaring `const DEFAULT_ENDPOINT` collided in
#     global scope and killed a Web Component's registration (SyntaxError);
#   - blog templates linked a stylesheet that no longer existed (404 x3);
#   - 8 admin JS files (~5.5k lines) sat unregistered and unserved for months.
#
# Checks, in order: JS syntax (node --check when node exists), registration <->
# filesystem in both directions, template links resolve to real sources, no
# classic <script> for files with top-level bindings, no window.Sp* namespace
# publishing, --sp- prefix on every custom property, no monolithic source
# files past 2x the standards cap.
set -uo pipefail

fail=0
warn() { printf 'check-frontend: WARN  %s\n' "$1"; }
err()  { printf 'check-frontend: ERROR %s\n' "$1"; fail=1; }

JS_DIR=storage/files/js
CSS_DIR=storage/files/css
TEMPLATES="services/web/templates storage/files/admin/templates"
REG_JS=extensions/web/site/src/assets/js_services.rs
REG_CSS=extensions/web/site/src/assets/css.rs

if command -v node >/dev/null 2>&1; then
    while IFS= read -r f; do
        if ! node --check "$f" >/dev/null 2>&1; then
            err "syntax error in $f"
            node --check "$f" 2>&1 | head -3
        fi
    done < <(find "$JS_DIR" -name '*.js')
else
    warn "node not found - JS syntax check skipped"
fi

python3 - <<'PY'
import re, sys, pathlib, glob

fail = False
def err(msg):
    global fail
    print(f'check-frontend: ERROR {msg}')
    fail = True
def warn(msg):
    print(f'check-frontend: WARN  {msg}')

js_sources = {str(p) for p in pathlib.Path('storage/files/js').rglob('*.js')}
css_sources = {str(p) for p in pathlib.Path('storage/files/css').rglob('*.css')}

reg_text = pathlib.Path('extensions/web/site/src/assets/js_services.rs').read_text()
reg_text += pathlib.Path('extensions/web/site/src/assets/css.rs').read_text()
for extra in glob.glob('extensions/web/*/src/**/*.rs', recursive=True):
    if 'assets' in extra:
        reg_text += pathlib.Path(extra).read_text()

registered_js = {n for n in re.findall(r'"([\w./-]+\.js)"', reg_text)}
registered_css = {n for n in re.findall(r'"([\w./-]+\.css)"', reg_text)}
def is_registered(path, names):
    base = path.split('storage/files/')[1].split('/', 1)[1]
    return any(n.endswith(base) or base.endswith(n) for n in names)

GENERATED = {'storage/files/css/admin-bundle.css'}
BUNDLED_PREFIX = 'storage/files/css/admin/'
for p in sorted(js_sources):
    if not is_registered(p, registered_js):
        err(f'{p} exists but is not registered in js_services.rs - it is never served')
for p in sorted(css_sources):
    if p in GENERATED or p.startswith(BUNDLED_PREFIX):
        continue
    if not is_registered(p, registered_css):
        err(f'{p} exists but is not registered in css.rs - it is never served')

def source_exists(name, root, extra_dirs):
    rel = name.split('/', 1)[1] if name.startswith(('js/', 'css/')) else name
    candidates = [pathlib.Path(root) / rel]
    candidates += [pathlib.Path(root) / d / rel for d in extra_dirs]
    return any(c.exists() for c in candidates)

for n in sorted(registered_js):
    if not source_exists(n, 'storage/files/js', ['services', 'utils']):
        err(f'js_services.rs registers {n} but no matching source under storage/files/js')
for n in sorted(registered_css):
    if not source_exists(n, 'storage/files/css', ['core', 'components']):
        err(f'css.rs registers {n} but no matching source under storage/files/css')

link_re = re.compile(r'\{\{(?:CSS|JS)_BASE_PATH\}\}/([\w./-]+)')
plain_re = re.compile(r'(?:href|src)="/(css|js)/([\w./-]+?\.(?:css|js))(?:\?[^"]*)?"')
script_re = re.compile(r'<script\b[^>]*src="[^"]*/(?:js)/([\w./-]+\.js)[^"]*"[^>]*>')
templates = []
for d in ('services/web/templates', 'storage/files/admin/templates'):
    templates += [p for p in pathlib.Path(d).rglob('*') if p.suffix in ('.html', '.hbs')]

top_binding = re.compile(r'^(?:const|let|class|function)\s', re.M)
for t in templates:
    text = t.read_text()
    for ref in link_re.findall(text):
        root = 'storage/files/css' if ref.endswith('.css') else 'storage/files/js'
        if not (pathlib.Path(root) / ref).exists():
            err(f'{t} links {ref} but no such file under {root}')
    for kind, ref in plain_re.findall(text):
        if not (pathlib.Path('storage/files') / kind / ref).exists():
            err(f'{t} links /{kind}/{ref} but no such source file')
    for m in re.finditer(r'<script\b([^>]*)>', text):
        attrs = m.group(1)
        src = re.search(r'src="[^"]*/js/([\w./-]+\.js)', attrs)
        if not src or 'type="module"' in attrs:
            continue
        f = pathlib.Path('storage/files/js') / src.group(1)
        if f.exists() and top_binding.search(f.read_text()):
            err(f'{t} loads {src.group(1)} as a classic script but it declares '
                'top-level bindings - collides in global scope; use type="module"')

for p in sorted(js_sources):
    text = pathlib.Path(p).read_text()
    if re.search(r'window\.Sp\w+\s*=', text):
        err(f'{p} publishes a window.Sp* namespace - export from the module instead')

bad_prop = re.compile(r'^\s*--(?!sp-|pi-|webkit)[a-z][\w-]*\s*:')
unprefixed = 0
for p in sorted(css_sources):
    if p in GENERATED:
        continue
    for i, line in enumerate(pathlib.Path(p).read_text().splitlines(), 1):
        if bad_prop.search(line):
            unprefixed += 1
if unprefixed:
    warn(f'{unprefixed} custom properties without an --sp- or --pi- prefix - '
         'rename toward the token convention as files are touched')

for p, cap, hard in [*((s, 150, 300) for s in js_sources), *((s, 200, 400) for s in css_sources)]:
    if p in GENERATED:
        continue
    n = len(pathlib.Path(p).read_text().splitlines())
    if n > hard * 2:
        warn(f'{p} is {n} lines - split it (guideline {cap}, decomposition overdue)')
    elif n > hard:
        warn(f'{p} is {n} lines (guideline {cap})')

sys.exit(1 if fail else 0)
PY
[ $? -ne 0 ] && fail=1

if [ "$fail" -ne 0 ]; then
    echo 'check-frontend: FAILED'
    exit 1
fi
echo 'check-frontend: ok'
