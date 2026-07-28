import { chainRail } from './pi-gate-view.js';
import { append } from './pi-terminal-dom.js';

/**
 * The chain that judged the call that follows.
 *
 * Rendered for every governed call, allow or deny. A gate only visible when it
 * blocks something reads as an error path; a gate visible on every call reads
 * as what it is.
 *
 * Held rather than drawn immediately. A rail on its own line says a chain ran
 * but not what it judged, and two of them in a row — one for the prompt, one
 * for the tool it triggered — read as the same thing rendered twice. So the
 * stages wait here for the row they belong to and are drawn inside it.
 */
export function policyStages(el, f) {
  el._railFor = f.stages || [];
  el._railDecision = f.decision_id || null;
}

/** The audit-trail URL for the decision the held rail is rendering. */
export function railTraceHref(el) {
  if (!el._railDecision || !el._conversationId) return null;
  return '/trace/' + encodeURIComponent(el._conversationId)
    + '#call-' + encodeURIComponent(el._railDecision);
}

/** Draw a held chain inline, or on its own line if nothing claimed it. */
export function railEl(el, stages, inline) {
  const rail = chainRail(stages, {
    compact: inline,
    traceHref: inline ? null : railTraceHref(el),
  });
  if (inline) return rail;
  const wrap = document.createElement('div');
  wrap.className = 'pi-rail-line';
  if (stages.some((st) => st.result === 'fail')) wrap.classList.add('is-denied');
  wrap.append(rail);
  return wrap;
}

/**
 * Nothing claimed the chain, so give it its own line.
 *
 * A held rail must never be silently dropped: the rail's whole claim to being
 * evidence is that it is a complete record of what ran.
 */
export function orphanRail(el) {
  if (!el._railFor) return;
  append(el, railEl(el, el._railFor, false));
  el._railFor = null;
  el._railDecision = null;
}
