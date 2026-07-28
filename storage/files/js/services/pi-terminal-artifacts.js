import { openArtifact } from './pi-artifact-overlay.js';

/**
 * The conversation's artifact shelf: every `tool_artifact` frame — live or
 * replayed — lands in `el._artifacts`, keyed by artifact id.
 *
 * The Map, not the DOM, is the record: transcript rows are trimmed once the
 * log grows past its cap, and a result must stay reachable after the row that
 * produced it has scrolled out of existence.
 */

/** A `tool_artifact` frame: remember it, badge the row, bump the chip. */
export function toolArtifact(el, f) {
  el._artifacts.set(f.artifact_id, f);

  // The producing row is still pending (the proxy answers before pi ends the
  // call), so badge it the same way takeRow matches: by the row's own data
  // attribute, oldest first.
  for (const row of el._toolRows.values()) {
    if (row.details.dataset.tool === f.tool_name && !row.details.dataset.artifact) {
      badgeRow(el, row, f);
      break;
    }
  }
  refreshChip(el);
}

function badgeRow(el, row, f) {
  row.details.dataset.artifact = f.artifact_id;
  const btn = document.createElement('button');
  btn.type = 'button';
  btn.className = 'pi-tool-artifact';
  btn.textContent = 'view result';
  // A <summary> click toggles the row; the button answers a different
  // question, so it must not also fold the arguments open.
  btn.addEventListener('click', (e) => {
    e.preventDefault();
    e.stopPropagation();
    openArtifact(el, f);
  });
  row.details.querySelector('summary').append(btn);
}

/** Chip + dropdown panel in the header — every result, newest last. */
export function wireArtifacts(el) {
  el._artChip.addEventListener('click', () => {
    const open = el._artPanel.hidden;
    if (open) renderPanel(el);
    el._artPanel.hidden = !open;
    el._artChip.setAttribute('aria-expanded', String(open));
  });
  document.addEventListener('click', (e) => {
    if (el._artPanel.hidden) return;
    if (!el._artChip.contains(e.target) && !el._artPanel.contains(e.target)) {
      el._artPanel.hidden = true;
      el._artChip.setAttribute('aria-expanded', 'false');
    }
  });
  document.addEventListener('keydown', (e) => {
    if (e.key === 'Escape' && !el._artPanel.hidden) {
      el._artPanel.hidden = true;
      el._artChip.setAttribute('aria-expanded', 'false');
    }
  });
}

export function resetArtifacts(el) {
  el._artifacts.clear();
  el._artPanel.hidden = true;
  refreshChip(el);
}

function refreshChip(el) {
  const n = el._artifacts.size;
  el._artWrap.hidden = n === 0;
  el._artCount.textContent = String(n);
}

function renderPanel(el) {
  el._artPanel.replaceChildren();
  const list = document.createElement('ul');
  list.className = 'pi-art-list';
  // Newest first: the result just produced is the one being looked for.
  [...el._artifacts.values()].reverse().forEach((f) => {
    const item = document.createElement('li');
    const btn = document.createElement('button');
    btn.type = 'button';
    btn.className = 'pi-art-item';
    const title = document.createElement('span');
    title.className = 'pi-art-item-title';
    title.textContent = f.title || f.tool_name;
    const kind = document.createElement('span');
    kind.className = 'pi-art-item-kind';
    kind.textContent = f.artifact_type + ' · ' + f.tool_name;
    btn.append(title, kind);
    btn.addEventListener('click', () => {
      el._artPanel.hidden = true;
      el._artChip.setAttribute('aria-expanded', 'false');
      openArtifact(el, f);
    });
    item.append(btn);
    list.append(item);
  });
  el._artPanel.append(list);
}
