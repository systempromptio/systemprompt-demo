'use strict';

/**
 * window.SpPiRender.markdown(text) -> DocumentFragment
 *
 * The markdown an agent actually emits, and nothing else: fenced code, inline
 * code, bold, italic, links, bullet and ordered lists, headings, and paragraphs.
 * No tables, no footnotes, no HTML passthrough.
 *
 * Hand-written rather than `marked` because the deployment target includes an
 * air-gapped image with no npm and no CDN reachable. The scope above is the
 * price of that, and it is a fair one: this text comes from a model answering
 * questions about a read-only workspace, not from a document pipeline.
 *
 * **Model output never touches innerHTML.** Every element is created, every leaf
 * is filled with textContent. The renderer cannot inject markup even if a
 * response is adversarial, which — since a prompt can be anything a visitor
 * types — is a security property and not a stylistic preference.
 *
 * Classic script, not a module: sp-pi-terminal.js takes no imports, so this
 * attaches to a namespace and the load order in the template stays trivial.
 */

/** Fence, heading, list item, and blank line. Everything else is prose. */
const FENCE = /^\s*```(\S*)\s*$/;
const HEADING = /^(#{1,6})\s+(.*)$/;
const BULLET = /^\s*[-*+]\s+(.*)$/;
const ORDERED = /^\s*\d+[.)]\s+(.*)$/;

/**
 * Inline spans, in the order they must be tried.
 *
 * Code first and unconditionally: backticks win over every other marker, so
 * `**not bold**` inside code stays literal. That ordering is the only reason a
 * single regex pass is safe here.
 */
const INLINE = [
  { cls: 'pi-code', re: /`([^`]+)`/, tag: 'code' },
  { cls: null, re: /\[([^\]]+)\]\((https?:\/\/[^\s)]+)\)/, tag: 'a' },
  { cls: 'pi-strong', re: /\*\*([^*]+)\*\*/, tag: 'strong' },
  { cls: 'pi-em', re: /(?:^|[^*])\*([^*]+)\*/, tag: 'em' },
];

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
function highlight(code, lang) {
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

/** Inline markup within one line of prose, appended to `parent`. */
function inline(parent, text) {
  let rest = text;
  let guard = 0;
  while (rest && guard++ < 500) {
    let best = null;
    for (const rule of INLINE) {
      const m = rule.re.exec(rest);
      if (!m) continue;
      // The italic rule deliberately consumes a leading non-* character so it
      // cannot fire inside a ** pair; offset past it when measuring position.
      const offset = m[0].indexOf('*') === 1 && rule.tag === 'em' ? 1 : 0;
      const index = m.index + offset;
      if (!best || index < best.index) {
        best = { index, rule, m, matched: m[0].slice(offset) };
      }
    }
    if (!best) break;
    if (best.index > 0) parent.append(document.createTextNode(rest.slice(0, best.index)));
    parent.append(inlineNode(best.rule, best.m));
    rest = rest.slice(best.index + best.matched.length);
  }
  if (rest) parent.append(document.createTextNode(rest));
}

function inlineNode(rule, m) {
  const el = document.createElement(rule.tag);
  if (rule.cls) el.className = rule.cls;
  if (rule.tag === 'a') {
    el.textContent = m[1];
    // Attributes rather than the reflected properties, so the hardening below is
    // visible in the DOM an auditor inspects and not only in this file. The URL
    // is already constrained to http(s) by the rule that matched it.
    el.setAttribute('href', m[2]);
    // Untrusted destination from model output: never let it reach window.opener,
    // and never let it inherit referrer.
    el.setAttribute('target', '_blank');
    el.setAttribute('rel', 'noopener noreferrer nofollow ugc');
  } else {
    el.textContent = m[1];
  }
  return el;
}

/** A fenced block plus a copy button, since a snippet exists to be taken. */
function codeBlock(lines, lang) {
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

/** Block-level parse. One pass, no intermediate AST. */
function markdown(text) {
  const frag = document.createDocumentFragment();
  const lines = String(text == null ? '' : text).split('\n');
  let i = 0;
  let para = [];
  let list = null;

  const flushPara = () => {
    if (!para.length) return;
    const p = document.createElement('p');
    inline(p, para.join(' '));
    frag.append(p);
    para = [];
  };
  const flushList = () => {
    if (list) frag.append(list.el);
    list = null;
  };

  while (i < lines.length) {
    const line = lines[i];
    const fence = FENCE.exec(line);
    if (fence) {
      flushPara();
      flushList();
      const lang = fence[1];
      const body = [];
      i += 1;
      // An unterminated fence is normal mid-stream: the closing ``` has not
      // arrived yet. Render what is there rather than dropping the block.
      while (i < lines.length && !FENCE.test(lines[i])) {
        body.push(lines[i]);
        i += 1;
      }
      i += 1;
      frag.append(codeBlock(body, lang));
      continue;
    }

    const heading = HEADING.exec(line);
    if (heading) {
      flushPara();
      flushList();
      // Offset by two: an h1 from a chat reply must not outrank the page's own
      // headings in the document outline a screen reader builds.
      const level = Math.min(heading[1].length + 2, 6);
      const h = document.createElement('h' + level);
      h.className = 'pi-heading';
      inline(h, heading[2]);
      frag.append(h);
      i += 1;
      continue;
    }

    const bullet = BULLET.exec(line);
    const ordered = bullet ? null : ORDERED.exec(line);
    if (bullet || ordered) {
      flushPara();
      const tag = bullet ? 'ul' : 'ol';
      if (!list || list.tag !== tag) {
        flushList();
        const el = document.createElement(tag);
        el.className = 'pi-list';
        list = { tag, el };
      }
      const li = document.createElement('li');
      inline(li, (bullet ? bullet[1] : ordered[1]));
      list.el.append(li);
      i += 1;
      continue;
    }

    if (!line.trim()) {
      flushPara();
      flushList();
      i += 1;
      continue;
    }

    flushList();
    para.push(line.trim());
    i += 1;
  }

  flushPara();
  flushList();
  return frag;
}

window.SpPiRender = { markdown, highlight };
