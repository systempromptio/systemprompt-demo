/**
 * highlight(code, lang) -> <code>, codeBlock(lines, lang) -> <div>
 *
 * Everything a fenced block needs: the token tinting, the chrome around it, and
 * the copy button, kept together because the three only ever appear together.
 *
 * **Model output never touches innerHTML.** Every element is created, every leaf
 * is filled with textContent — the same guarantee the markdown renderer makes,
 * and it has to hold here too since a fence carries model text verbatim.
 */

/**
 * Four token classes, not a language grammar.
 *
 * A real highlighter needs a parser per language and would be the largest file
 * in the repo. Comments, strings, numbers, and a shared keyword set cover what
 * makes a snippet readable at a glance, and mis-tinting an identifier in an
 * unusual language costs nothing. Reuses the --sp-syntax-* palette the blog's
 * code blocks already use, so a snippet here looks like a snippet there.
 */
const KEYWORDS = new RegExp('\\b(' + [
  'fn', 'let', 'const', 'mut', 'pub', 'struct', 'enum', 'impl', 'trait', 'use',
  'mod', 'match', 'if', 'else', 'for', 'while', 'loop', 'return', 'async',
  'await', 'self', 'Some', 'None', 'Ok', 'Err', 'true', 'false', 'function',
  'var', 'class', 'new', 'import', 'export', 'from', 'def', 'class', 'try',
  'catch', 'throw', 'typeof', 'null', 'undefined', 'this',
].join('|') + ')\\b');

const COMMENT = /(\/\/[^\n]*|#[^\n]*|\/\*[\s\S]*?\*\/)/;
const STRING = /("(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*')/;
const NUMBER = /\b(\d+(?:\.\d+)?)\b/;

/** Render one block of source into a <code>, tinting four token classes. */
export function highlight(code, lang) {
  const el = document.createElement('code');
  if (lang) el.dataset.lang = lang;
  // Longest-match-first over the whole block: a keyword inside a string or a
  // comment must not be tinted as a keyword, so those two are tried first.
  let rest = code;
  while (rest) {
    const hit = firstOf(rest, [
      { re: COMMENT, cls: 'pi-tok-comment' },
      { re: STRING, cls: 'pi-tok-string' },
      { re: KEYWORDS, cls: 'pi-tok-keyword' },
      { re: NUMBER, cls: 'pi-tok-number' },
    ]);
    if (!hit) {
      el.append(document.createTextNode(rest));
      break;
    }
    if (hit.index > 0) el.append(document.createTextNode(rest.slice(0, hit.index)));
    const span = document.createElement('span');
    span.className = hit.cls;
    span.textContent = hit.text;
    el.append(span);
    rest = rest.slice(hit.index + hit.text.length);
  }
  return el;
}

/** The earliest match among several patterns, or null. */
function firstOf(s, patterns) {
  let best = null;
  for (const p of patterns) {
    const m = p.re.exec(s);
    if (!m) continue;
    // Keywords match with a word boundary, so m[1] and m[0] are the same span;
    // the others capture inside delimiters we still want to keep.
    const text = m[0];
    const index = m.index;
    if (!best || index < best.index) best = { index, text, cls: p.cls };
  }
  return best;
}

/** A fenced block plus a copy button, since a snippet exists to be taken. */
export function codeBlock(lines, lang) {
  const wrap = document.createElement('div');
  wrap.className = 'pi-codeblock';
  const bar = document.createElement('div');
  bar.className = 'pi-codeblock-bar';
  const label = document.createElement('span');
  label.className = 'pi-codeblock-lang';
  label.textContent = lang || 'text';
  const copy = document.createElement('button');
  copy.type = 'button';
  copy.className = 'pi-copy';
  copy.textContent = 'copy';
  const source = lines.join('\n');
  copy.addEventListener('click', () => {
    void copyText(source, copy);
  });
  bar.append(label, copy);
  const pre = document.createElement('pre');
  pre.append(highlight(source, lang));
  wrap.append(bar, pre);
  return wrap;
}

/**
 * Clipboard write with a visible outcome.
 *
 * navigator.clipboard is unavailable on insecure origins, which a self-hosted
 * deployment on plain http will be, so a failure is expected rather than
 * exceptional and has to say so instead of doing nothing.
 */
async function copyText(text, btn) {
  try {
    await navigator.clipboard.writeText(text);
    btn.textContent = 'copied';
  } catch (_) {
    btn.textContent = 'copy failed';
  }
  setTimeout(() => { btn.textContent = 'copy'; }, 1600);
}
