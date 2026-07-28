/** The telemetry tablist of <sp-auth-pane>: wiring and selection. */

export function wireTabs(pane) {
  pane._tabs = Array.from(pane.querySelectorAll('.pane-tabs--stats .pane-tab'));
  pane._panels = Array.from(pane.querySelectorAll('.pane-panel'));
  if (!pane._tabs.length) return;
  pane._tabs.forEach((tab) => {
    tab.addEventListener('click', () => selectTab(pane, tab.dataset.tab));
  });
  pane.querySelector('.pane-tabs--stats').addEventListener('keydown', (e) => {
    if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(e.key)) return;
    e.preventDefault();
    const tabs = pane._tabs;
    const current = tabs.findIndex((t) => t.dataset.tab === pane._activeTab);
    let next = current;
    if (e.key === 'ArrowLeft') next = (current - 1 + tabs.length) % tabs.length;
    else if (e.key === 'ArrowRight') next = (current + 1) % tabs.length;
    else if (e.key === 'Home') next = 0;
    else next = tabs.length - 1;
    selectTab(pane, tabs[next].dataset.tab);
    tabs[next].focus();
  });
  // Survives a re-render (sign-out/in, re-auth): the last chosen tab is
  // restored rather than snapping back to Overview.
  selectTab(pane, pane._activeTab || 'overview');
}

export function selectTab(pane, id) {
  pane._activeTab = id;
  pane._tabs.forEach((tab) => {
    const active = tab.dataset.tab === id;
    tab.classList.toggle('is-active', active);
    tab.setAttribute('aria-selected', active ? 'true' : 'false');
    tab.tabIndex = active ? 0 : -1;
  });
  pane._panels.forEach((p) => { p.hidden = p.id !== 'ap-panel-' + id; });
  // The Platform panel has no tab — it opens from the admin badge, which
  // mirrors the selection state the tablist would otherwise carry.
  const badge = pane.querySelector('.pane-badge--admin');
  if (badge) badge.classList.toggle('is-active', id === 'platform');
}
