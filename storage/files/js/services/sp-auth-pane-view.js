/** The tabbed telemetry markup for <sp-auth-pane>. Pure strings, no state. */

export function statHtml(key, label, initial) {
  return '<div class="pane-stat"><dt>' + label + '</dt>'
    + '<dd data-stat="' + key + '">' + initial + '</dd></div>';
}

const TABS = [
  { id: 'overview', label: 'Overview' },
  { id: 'traffic', label: 'Traffic' },
  { id: 'usage', label: 'Usage' },
  { id: 'activity', label: 'Activity' },
  { id: 'governance', label: 'Governance' },
  // Hidden until the pulse endpoint answers with the admin detail block. The
  // server never tells the client its role — the payload shape is the tier —
  // so the tab appears only once there is admin data to put behind it.
  { id: 'platform', label: 'Platform', hidden: true },
];

export function tabsHtml() {
  return '<div class="pane-tabs pane-tabs--stats" role="tablist" aria-label="Session telemetry">'
    + TABS.map((t, i) => {
      const active = i === 0;
      return '<button type="button" class="pane-tab' + (active ? ' is-active' : '')
        + '" role="tab" id="ap-tab-' + t.id + '" aria-controls="ap-panel-' + t.id
        + '" aria-selected="' + active + '" tabindex="' + (active ? '0' : '-1')
        + '" data-tab="' + t.id + '"' + (t.hidden ? ' hidden' : '') + '>' + t.label
        + (t.id === 'governance'
          ? '<span class="pane-tab-chip" data-role="gov-chip" hidden></span>'
          : '')
        + '</button>';
    }).join('')
    + '</div>';
}

export function panelHtml(id, inner, hidden) {
  return '<section class="pane-panel" role="tabpanel" id="ap-panel-' + id
    + '" aria-labelledby="ap-tab-' + id + '" tabindex="0"' + (hidden ? ' hidden' : '') + '>'
    + inner + '</section>';
}

/**
 * The default view answers "what is going on" in one screen: the headline
 * numbers large, the model in one line, the pipeline as pips, and the last
 * few decisions. Everything on it is repeated in full on another tab.
 */
export function overviewHtml() {
  return '<dl class="pane-stats pane-stats--hero">'
    + statHtml('requests', 'Requests', '0')
    + statHtml('tools', 'Tool calls', '0')
    + statHtml('blocked', 'Blocked', '0')
    + statHtml('cost', 'Session cost', '$0')
    + '</dl>'
    + '<p class="pane-model-line"><span data-stat="model">—</span>'
    + '<span class="pane-model-sep">·</span><span data-stat="provider">—</span>'
    + '<span class="pane-model-sep">·</span><span data-stat="route">—</span></p>'
    + '<div class="pane-stage-mini" data-role="stage-mini"'
    + ' aria-label="policy pipeline"></div>'
    + '<div class="pane-strip" data-role="req-strip" hidden></div>'
    + '<p class="pane-ribbon" data-role="platform-ribbon" hidden></p>'
    + '<section class="pane-section pane-section--feed">'
    + '<h3 class="pane-h3">Latest decisions '
    + '<button type="button" class="pane-link pane-link--sm" data-role="view-gov">'
    + 'view all</button></h3>'
    + '<ol class="pane-feed pane-feed--preview" data-role="feed-preview"></ol>'
    + '</section>';
}

export function trafficHtml() {
  return sectionHtml('Traffic', 'traffic', [
    statHtml('requests', 'Requests', '0'),
    statHtml('tools', 'Tool calls', '0'),
    statHtml('blocked', 'Blocked', '0'),
    statHtml('errors', 'Errors', '0'),
    statHtml('successRate', 'Success', '—'),
  ])
  + '<section class="pane-section">'
  + '<h3 class="pane-h3">Latency</h3>'
  + '<div class="pane-lat">'
  + latencyRowHtml('latency', 'p50')
  + latencyRowHtml('latency95', 'p95')
  + latencyRowHtml('latencyLast', 'Last turn')
  + '</div>'
  + '</section>'
  + '<section class="pane-section" data-role="lat-chart-section" hidden>'
  + '<h3 class="pane-h3">Latency over time '
  + '<span class="pane-h3-sub">p95 per bucket, red where a request failed</span></h3>'
  + '<div class="pane-chart-host" data-role="lat-chart"></div>'
  + '</section>'
  + '<div data-role="ok-bar" hidden></div>'
  + '<section class="pane-section pane-section--feed">'
  + '<h3 class="pane-h3">Requests <span class="pane-h3-sub">newest first, each with '
  + 'its audit trail</span></h3>'
  + '<ol class="pane-reqs" data-role="req-list">'
  + '<li class="pane-feed-empty">No requests yet — send a prompt in the terminal and '
  + 'each one lands here with its latency, cost, and audit trail.</li>'
  + '</ol>'
  + '</section>';
}

/**
 * The Activity tab: this account across every conversation. The per-session
 * tabs answer "what is this run doing"; this one answers "what have I been
 * doing here", which is the record the governance spine exists to keep.
 */
export function activityHtml() {
  return '<section class="pane-section">'
    + '<h3 class="pane-h3">All time</h3>'
    + '<dl class="pane-stats" data-role="act-totals"></dl>'
    + '</section>'
    + '<section class="pane-section pane-section--feed">'
    + '<h3 class="pane-h3">Conversations <span class="pane-h3-sub">cost bars share '
    + 'one scale</span></h3>'
    + '<ol class="pane-convs" data-role="act-list">'
    + '<li class="pane-feed-empty">No conversations yet — start one in the terminal '
    + 'and its full audit trail appears here.</li>'
    + '</ol>'
    + '</section>'
    + '<section class="pane-section" data-role="act-tools-section" hidden>'
    + '<h3 class="pane-h3">Most used tools</h3>'
    + '<div class="pane-mix" data-role="act-tools"></div>'
    + '</section>';
}

export function latencyRowHtml(key, label) {
  return '<div class="pane-lat-row"><span class="pane-lat-label">' + label + '</span>'
    + '<div class="pane-bar"><span data-bar="' + key + '"></span></div>'
    + '<span class="pane-lat-val" data-stat="' + key + '">—</span></div>';
}

export function usageHtml() {
  return sectionHtml('Tokens', 'tokens', [
    statHtml('tokensIn', 'Input', '0'),
    statHtml('tokensOut', 'Output', '0'),
    statHtml('cacheRead', 'Cache read', '0'),
    statHtml('cacheWrite', 'Cache written', '0'),
  ])
  + sectionHtml('Cost', 'cost-section', [
    statHtml('cost', 'This session', '$0'),
    statHtml('costPer', 'Per request', '$0'),
  ])
  + sectionHtml('Model &amp; route', 'route', [
    statHtml('model', 'Model', '—'),
    statHtml('requested', 'Requested', 'as served'),
    statHtml('provider', 'Provider', '—'),
    statHtml('route', 'Gateway route', '—'),
    statHtml('cache', 'Cache hits', '0%'),
  ])
  + '<section class="pane-section" data-role="mix-section" hidden>'
  + '<h3 class="pane-h3">Model mix</h3>'
  + '<div class="pane-mix" data-role="mix"></div>'
  + '</section>';
}

export function governanceHtml() {
  // The stage list, not a tile grid: these are four sequential checks and
  // the order they run in is part of what is being shown.
  return '<section class="pane-section">'
    + '<h3 class="pane-h3">Policy pipeline '
    + '<span class="pane-h3-sub" data-role="stage-sub"></span></h3>'
    + '<ol class="pane-stages" data-role="stages"></ol>'
    + '</section>'
    + '<section class="pane-section pane-section--feed">'
    + '<h3 class="pane-h3">Governance <span class="pane-h3-sub" data-role="feed-count"></span></h3>'
    + '<ol class="pane-feed" data-role="feed">'
    + '<li class="pane-feed-empty">Ask the agent to read a file. Every decision it '
    + 'triggers is recorded here.</li>'
    + '</ol>'
    + '</section>';
}

/**
 * The admin-only Platform tab: the deployment counted across everyone.
 * Rendered for every signed-in visitor but reachable only once the tab is
 * revealed, which happens when the pulse arrives carrying the admin detail.
 */
export function platformHtml() {
  return '<section class="pane-section pane-section--pulse" data-role="pulse">'
    + '<h3 class="pane-h3">Across the platform '
    + '<span class="pane-h3-sub" data-role="pulse-window"></span></h3>'
    + '<dl class="pane-stats pane-stats--pulse" data-role="pulse-stats"></dl>'
    + '<p class="pane-pulse-note" data-role="pulse-models"></p>'
    + '<p class="pane-pulse-note" data-role="pulse-all-time"></p>'
    + '</section>';
}

/** One labelled group of tiles. */
export function sectionHtml(title, role, tiles) {
  return '<section class="pane-section">'
    + '<h3 class="pane-h3">' + title + '</h3>'
    + '<dl class="pane-stats" data-role="' + role + '">' + tiles.join('') + '</dl>'
    + '</section>';
}
