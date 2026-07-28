import { pretty } from './pi-format.js';
import { EXPLAIN, chainRail } from './pi-gate-view.js';

/**
 * The pieces the approval card and the auto-approved record are both built
 * from. The two rows show the same facts — tool, arguments, the chain that
 * cleared it — and differ only in whether a question follows, so the facts
 * are constructed once, here.
 */

/**
 * Who decided, and when — the by-line every settled approval carries. A named
 * human gets an initials avatar; a system path (timeout, auto-approve) gets a
 * muted, avatar-less variant so machine decisions never dress up as a person.
 */
export function attributionStamp({ name, at, actor, action }) {
  const stamp = document.createElement('div');
  stamp.className = 'pi-attribution';
  stamp.dataset.actor = actor === 'system' ? 'system' : 'user';
  if (actor !== 'system') {
    const avatar = document.createElement('span');
    avatar.className = 'pi-attribution-avatar';
    avatar.textContent = initials(name);
    avatar.setAttribute('aria-hidden', 'true');
    stamp.append(avatar);
  }
  const text = document.createElement('span');
  text.className = 'pi-attribution-text';
  const who = document.createElement('strong');
  who.textContent = name || 'system';
  text.append(who, document.createTextNode(' ' + (action || 'approved')));
  stamp.append(text);
  if (at) {
    const time = document.createElement('time');
    time.className = 'pi-attribution-at';
    time.dateTime = at;
    time.textContent = ' at ' + new Date(at).toLocaleTimeString();
    stamp.append(time);
  }
  return stamp;
}

function initials(name) {
  const parts = (name || '').trim().split(/\s+/).filter(Boolean).slice(0, 2);
  return parts.map((w) => w[0].toUpperCase()).join('') || '?';
}

/** One small true fact, as a pill. */
export function detailChip(text) {
  const chip = document.createElement('span');
  chip.className = 'pi-detail-chip';
  chip.textContent = text;
  return chip;
}

/** The tool name over one line of context. */
export function toolTitle(toolName, subText) {
  const title = document.createElement('div');
  title.className = 'pi-approval-title';
  const tool = document.createElement('strong');
  tool.textContent = toolName;
  const sub = document.createElement('span');
  sub.className = 'pi-approval-sub';
  sub.textContent = subText;
  title.append(tool, sub);
  return title;
}

/**
 * Args on the left, the cleared chain on the right — the two facts a decision
 * rests on, side by side instead of stacked into a banner. The chain is shown
 * compact: the dots carry the verdicts and the names arrive on hover or focus.
 */
export function approvalGrid(frame, clearedText) {
  const chain = frame.policy_chain || [];
  const cleared = document.createElement('div');
  cleared.className = 'pi-approval-cleared';
  const label = document.createElement('span');
  label.className = 'pi-approval-cleared-label';
  label.textContent = clearedText;
  cleared.append(label, chainRail(
    chain.map((p) => ({ policy: p, result: 'pass', detail: EXPLAIN[p] || '' })),
    { compact: true },
  ));

  const grid = document.createElement('div');
  grid.className = 'pi-approval-grid';
  grid.append(kvArgs(frame.tool_input), cleared);
  return grid;
}

/**
 * Small true facts, stated as chips: how much of the chain passed, plus
 * whatever the caller knows about the outcome. All come from the frame, not
 * from copy.
 */
export function metaRow(chain, extras) {
  const meta = document.createElement('div');
  meta.className = 'pi-detail-row';
  if (chain.length) {
    meta.append(detailChip(chain.length + '/' + chain.length + ' policies passed'));
  }
  extras.forEach((text) => meta.append(detailChip(text)));
  return meta;
}

/**
 * A tool's arguments as a key/value ledger, when they are flat enough to be
 * one — `TO board@acme.com` reads at a glance where a JSON blob has to be
 * parsed by eye. Nested or non-object input falls back to the exact
 * pretty-printed JSON: a card asking for a judgement must never summarise
 * away what it is asking about.
 */
export function kvArgs(input) {
  // A tool with no parameters is a fact worth stating, not a `{}` to decode.
  const empty = input == null || (typeof input === 'object' && !Array.isArray(input)
    && !Object.keys(input).length);
  if (empty) {
    const none = document.createElement('p');
    none.className = 'pi-approval-noargs';
    none.textContent = 'no arguments';
    return none;
  }
  const flat = typeof input === 'object' && !Array.isArray(input)
    && Object.values(input).every((v) => ['string', 'number', 'boolean'].includes(typeof v));
  if (!flat) {
    const pre = document.createElement('pre');
    pre.className = 'pi-approval-input';
    pre.textContent = pretty(input);
    return pre;
  }
  const dl = document.createElement('dl');
  dl.className = 'pi-approval-kv';
  Object.entries(input).forEach(([k, v]) => {
    const dt = document.createElement('dt');
    dt.textContent = k;
    const dd = document.createElement('dd');
    dd.textContent = String(v);
    dl.append(dt, dd);
  });
  return dl;
}
