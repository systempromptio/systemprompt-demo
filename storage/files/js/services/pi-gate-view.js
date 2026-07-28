/**
 * The governance chain, rendered.
 *
 * This is the part of the terminal that no pty can produce. The stream carries a
 * typed `policy_stages` frame naming every policy that ran, in the order it ran,
 * with its real result — so the widget can show the pipeline resolving rather
 * than only reporting its verdict.
 *
 * The load-bearing detail is what happens on a deny: stages after the failure
 * are `skip`, and a skipped pip stays unlit. A visitor watching a blocked call
 * sees the chain stop. That is the whole demonstration, and it is why `skip` is
 * a distinct state here and in the Rust frame rather than being folded into a
 * boolean.
 */

/** Per-pip reveal delay. Long enough to read left-to-right, short enough not to
 *  delay an operator who is waiting to answer an approval card. */
const STAGGER_MS = 90;

const GLYPH = { pass: '✓', fail: '✗', skip: '·' };

/**
 * Human wording for the policies shipped in `policies/mod.rs`, plus the
 * caller-side confinement check.
 *
 * A lookup, not a rename: the pip is always labelled with the real policy id
 * from the frame, and this only supplies the sentence under it. A policy added
 * upstream therefore still renders — unlabelled prose is a cosmetic gap, whereas
 * a missing pip would be a lie about what ran.
 */
export const EXPLAIN = {
  scope_check: 'the agent’s scope permits this tool',
  secret_scan: 'no credential pattern in the arguments',
  tool_blocklist: 'the tool is not blocked for this deployment',
  rate_limit: 'the conversation is inside its call budget',
  workspace_scope: 'every path stays inside the session workspace',
  human_approval: 'a person answered',
};

/** Whether to animate at all. Read per call, so an OS-level change mid-session
 *  is honoured without a reload. */
export function motionOk() {
  return !window.matchMedia('(prefers-reduced-motion: reduce)').matches;
}

/** Milliseconds as a reader sees them: sub-millisecond checks say so instead of
 *  rounding to a `0ms` that reads as "not measured". */
function formatMs(ms) {
  if (typeof ms !== 'number' || !isFinite(ms) || ms <= 0) return '';
  return ms < 1 ? '<1ms' : Math.round(ms) + 'ms';
}

/**
 * The chain rail: one pip per stage, in evaluation order.
 *
 * `stages` is the frame's array verbatim — this function never invents, reorders,
 * or filters an entry, because the rail's only claim to being evidence is that it
 * is a direct rendering of what the evaluation reported.
 */
export function chainRail(stages, opts) {
  const animate = (!opts || opts.animate !== false) && motionOk();
  // Compact is the inline form, drawn inside the row the chain judged. The pips
  // shrink to their dots and the policy names are revealed on hover or focus —
  // attached to its subject, the rail no longer has to name it in full.
  const compact = !!(opts && opts.compact);
  const rail = document.createElement('div');
  rail.className = compact ? 'pi-rail pi-rail--compact' : 'pi-rail';
  rail.setAttribute('role', 'list');
  rail.setAttribute('aria-label', 'Governance chain');

  stages.forEach((stage, n) => {
    const pip = document.createElement('span');
    pip.className = 'pi-pip';
    pip.dataset.result = stage.result;
    pip.setAttribute('role', 'listitem');

    const dot = document.createElement('span');
    dot.className = 'pi-pip-dot';
    dot.textContent = GLYPH[stage.result] || GLYPH.skip;
    dot.setAttribute('aria-hidden', 'true');

    const name = document.createElement('span');
    name.className = 'pi-pip-name';
    name.textContent = stage.policy;

    pip.append(dot, name);

    const ms = formatMs(stage.duration_ms);
    if (ms) {
      const took = document.createElement('span');
      took.className = 'pi-pip-ms';
      took.textContent = ms;
      took.setAttribute('aria-hidden', 'true');
      pip.append(took);
    }

    // The screen-reader text says the outcome in words; the glyph and the colour
    // are both hidden from it, so nothing depends on either being perceived.
    const sr = document.createElement('span');
    sr.className = 'sp-sr-only';
    const verdict = stage.result === 'pass' ? 'passed'
      : stage.result === 'fail' ? 'failed' : 'not run';
    sr.textContent = ' ' + stage.policy + ' ' + verdict
      + (ms ? ' in ' + ms : '')
      + (stage.detail ? ': ' + stage.detail : '') + '. ';
    pip.append(sr);

    // The detail is the policy's own wording, straight from the audit spine, so
    // the tooltip and the trace row cannot disagree about why.
    const why = stage.detail || EXPLAIN[stage.policy] || '';
    if (why) pip.title = stage.policy + (ms ? ' (' + ms + ')' : '') + ' — ' + why;

    if (animate) {
      pip.classList.add('is-pending');
      setTimeout(() => pip.classList.remove('is-pending'), n * STAGGER_MS);
    }
    rail.append(pip);
  });

  // The way out of the summary and into the evidence: every rail row can open
  // the audit trail for the exact decision it is rendering.
  if (opts && opts.traceHref) {
    const audit = document.createElement('a');
    audit.className = 'pi-rail-audit';
    audit.href = opts.traceHref;
    audit.target = '_blank';
    audit.rel = 'noopener';
    audit.textContent = 'audit →';
    audit.setAttribute('aria-label', 'Open the audit trail for this call');
    rail.append(audit);
  }

  return rail;
}
