/**
 * CSS-only chart builders shared across <sp-auth-pane> tabs.
 *
 * Bars, not lines: every series here is discrete counts or per-request
 * samples, and a line between two buckets implies values that were never
 * measured. No library — each chart is a row of spans whose heights carry
 * the data, which is the same idiom the admin pulse sparkline established.
 */

/**
 * A column per point, scaled to the largest. `hot` points render in the
 * denial colour. Returns a detached element; the caller owns placement.
 *
 * @param {Array<{value:number, hot?:boolean, title?:string}>} points
 * @param {string} ariaLabel
 */
export function columnChart(points, ariaLabel) {
  const chart = document.createElement('div');
  chart.className = 'pane-chart';
  chart.setAttribute('role', 'img');
  chart.setAttribute('aria-label', ariaLabel);
  const max = Math.max(1, ...points.map((p) => Number(p.value) || 0));
  points.forEach((p) => {
    const bar = document.createElement('span');
    bar.className = 'pane-chart-col';
    bar.style.height = Math.max(4, Math.round(((Number(p.value) || 0) / max) * 100)) + '%';
    if (p.hot) bar.dataset.hot = '1';
    if (p.title) bar.title = p.title;
    chart.append(bar);
  });
  return chart;
}

/**
 * One horizontal bar split into an ok segment and a bad segment, with the
 * rate spelled out beside it — the success-rate picture in the pane's own
 * bar language.
 */
export function splitBar(ok, bad, okLabel, badLabel) {
  const wrap = document.createElement('div');
  wrap.className = 'pane-split';
  const total = Math.max(1, (Number(ok) || 0) + (Number(bad) || 0));
  const bar = document.createElement('div');
  bar.className = 'pane-split-bar';
  bar.setAttribute('role', 'img');
  bar.setAttribute('aria-label', okLabel + ' ' + ok + ', ' + badLabel + ' ' + bad);
  const okSeg = document.createElement('span');
  okSeg.className = 'pane-split-ok';
  okSeg.style.width = Math.round(((Number(ok) || 0) / total) * 100) + '%';
  const badSeg = document.createElement('span');
  badSeg.className = 'pane-split-bad';
  badSeg.style.width = Math.round(((Number(bad) || 0) / total) * 100) + '%';
  bar.append(okSeg, badSeg);
  const legend = document.createElement('span');
  legend.className = 'pane-split-legend';
  legend.textContent = ok + ' ' + okLabel + ' · ' + bad + ' ' + badLabel;
  wrap.append(bar, legend);
  return wrap;
}

/** A share bar in the model-mix idiom, scaled against a caller-chosen max. */
export function shareBar(value, max, ariaLabel) {
  const bar = document.createElement('div');
  bar.className = 'pane-bar';
  bar.setAttribute('role', 'img');
  bar.setAttribute('aria-label', ariaLabel);
  const fill = document.createElement('span');
  fill.style.width = Math.max(0, Math.min(100,
    Math.round(((Number(value) || 0) / Math.max(1, max)) * 100))) + '%';
  bar.append(fill);
  return bar;
}
