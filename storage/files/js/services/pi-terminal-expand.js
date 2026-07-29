/**
 * The expand toggle: the terminal takes the whole viewport on a solid field,
 * or gives it back. All geometry lives in pi-terminal-expand.css — this only
 * moves the two classes and keeps the button and Escape in agreement.
 */

export function wireExpand(el) {
  const btn = el.querySelector('[data-role="expand"]');
  const label = el.querySelector('[data-role="expand-label"]');

  const set = (expanded) => {
    el.classList.toggle('pi-terminal--expanded', expanded);
    document.body.classList.toggle('pi-terminal-expanded', expanded);
    btn.setAttribute('aria-pressed', String(expanded));
    btn.title = expanded
      ? 'Return the terminal to the page'
      : 'Expand the terminal to fill the page';
    label.textContent = expanded ? 'Collapse' : 'Expand';
    if (expanded) {
      document.addEventListener('keydown', onKey);
    } else {
      document.removeEventListener('keydown', onKey);
    }
  };

  const onKey = (e) => {
    // The conversation panel's own Escape closes it first; only a bare Escape
    // collapses the terminal, so the two dismissals never fight over one key.
    if (e.key === 'Escape' && (!el._convPanel || el._convPanel.hidden)) {
      set(false);
    }
  };

  btn.addEventListener('click', () => {
    set(!el.classList.contains('pi-terminal--expanded'));
  });
}
