/**
 * What policy did, as <sp-auth-pane> shows it: the four-stage pipeline, the
 * Governance tab's count chip, and the decision feed.
 */

import { feedItem } from './sp-auth-pane-helpers.js';

/** The pipeline, before any poll has told us what it did. */
export const IDLE_STAGES = [
  { id: 'secret_scan', label: 'Secret scan', passed: 0, failed: 0, active: false },
  { id: 'scope_check', label: 'Scope check', passed: 0, failed: 0, active: false },
  { id: 'tool_blocklist', label: 'Tool blocklist', passed: 0, failed: 0, active: false },
  { id: 'rate_limit', label: 'Rate limit', passed: 0, failed: 0, active: false },
];

/**
 * The Governance tab's count chip lives in the always-visible tablist, so a
 * denial registers even while that panel is hidden.
 */
export function applyGovChip(pane, count, denials) {
  if (!pane._govChip) return;
  pane._govChip.hidden = !count;
  pane._govChip.textContent = String(count);
  pane._govChip.dataset.alert = denials > 0 ? '1' : '0';
  const tab = pane.querySelector('#ap-tab-governance');
  if (tab) {
    tab.setAttribute('aria-label', 'Governance, ' + count + ' events'
      + (denials ? ', ' + denials + ' blocked' : ''));
  }
}

/**
 * The four checks, in the order they run.
 *
 * A stage that has never evaluated anything is dimmed rather than hidden:
 * "four checks run on every call, none has tripped" is the claim, and a list
 * that grows from one row to four as things happen argues the opposite.
 */
export function applyStages(pane, stages) {
  if (!pane._stages) return;
  const blocked = stages.reduce((n, st) => n + (st.failed || 0), 0);
  pane._stageSub.textContent = blocked
    ? blocked + (blocked === 1 ? ' block' : ' blocks')
    : stages.length + ' checks per call';

  pane._stages.innerHTML = '';
  stages.forEach((st) => {
    const li = document.createElement('li');
    li.className = 'pane-stage';
    li.dataset.hot = st.failed > 0 ? '1' : '0';
    li.dataset.active = st.active ? '1' : '0';

    const name = document.createElement('span');
    name.className = 'pane-stage-name';
    name.textContent = st.label || st.id;

    const tally = document.createElement('span');
    tally.className = 'pane-stage-tally';
    tally.textContent = st.failed > 0
      ? st.passed + ' passed · ' + st.failed + ' blocked'
      : (st.active ? st.passed + ' passed' : 'idle');

    li.append(name, tally);
    pane._stages.append(li);
  });
  applyStageMini(pane, stages);
}

/**
 * The Overview's one-line echo of the pipeline: four named pips, red where
 * a stage has blocked something. The full tallies live on the Governance
 * tab; this exists so the pipeline is present on the default view at all.
 */
export function applyStageMini(pane, stages) {
  if (!pane._stageMini) return;
  pane._stageMini.innerHTML = '';
  stages.forEach((st) => {
    const pip = document.createElement('span');
    pip.className = 'pane-stage-pip';
    pip.dataset.hot = st.failed > 0 ? '1' : '0';
    pip.dataset.active = st.active ? '1' : '0';
    pip.textContent = st.label || st.id;
    if (st.failed > 0) pip.title = st.failed + ' blocked';
    pane._stageMini.append(pip);
  });
}

export function renderFeed(pane, events) {
  pane._feedCount.textContent = events.length ? events.length + ' recorded' : '';
  if (!events.length) return;
  pane._feed.innerHTML = '';
  // Newest first: the pane is short, and the thing that just happened is the
  // thing being watched for.
  events.slice(-40).reverse().forEach((e) => pane._feed.append(feedItem(e)));
  syncFeedPreview(pane);
}

export function pushFeed(pane, e) {
  const empty = pane._feed.querySelector('.pane-feed-empty');
  if (empty) empty.remove();
  pane._feed.prepend(feedItem(e));
  syncFeedPreview(pane);
}

/** The Overview shows the three newest decisions; the full list is a tab away. */
export function syncFeedPreview(pane) {
  if (!pane._feedPreview || !pane._feed) return;
  pane._feedPreview.innerHTML = '';
  const items = Array.from(pane._feed.children)
    .filter((li) => !li.classList.contains('pane-feed-empty'))
    .slice(0, 3);
  if (!items.length) {
    const li = document.createElement('li');
    li.className = 'pane-feed-empty';
    li.textContent = 'Ask the agent to read a file — every decision lands here live.';
    pane._feedPreview.append(li);
    return;
  }
  items.forEach((li) => pane._feedPreview.append(li.cloneNode(true)));
}
