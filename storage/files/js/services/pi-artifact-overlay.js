import { pretty } from './pi-format.js';
import { getJson } from './pi-transport.js';

/**
 * The artifact viewer — a floating panel over the terminal, never a modal.
 *
 * The transcript stays interactive while it is open for the same reason the
 * approval queue is not a dialog: the model keeps working, and the viewer is
 * a window onto one finished result, not a question blocking the next one.
 *
 * Preview is the server's own render of the stored artifact, loaded in a
 * sandboxed iframe — the same HTML an MCP host would show, not a second
 * renderer to keep honest. Raw is the stored row, fetched once on first use.
 */

function artifactUrl(el, id) {
  return el._endpoint + '/artifacts/' + encodeURIComponent(id)
    + '?token=' + encodeURIComponent(el._token);
}

export function openArtifact(el, meta) {
  closeArtifact(el);

  const overlay = document.createElement('div');
  overlay.className = 'pi-artifact-overlay';
  overlay.setAttribute('role', 'region');
  overlay.setAttribute('aria-label', 'Tool result: ' + (meta.title || meta.tool_name));

  const head = document.createElement('div');
  head.className = 'pi-artifact-head';
  const title = document.createElement('strong');
  title.className = 'pi-artifact-title';
  title.textContent = meta.title || meta.tool_name;
  const type = document.createElement('span');
  type.className = 'pi-detail-chip';
  type.textContent = meta.artifact_type;

  const tabs = document.createElement('div');
  tabs.className = 'pi-artifact-tabs';
  const previewTab = tabBtn('Preview', true);
  const rawTab = tabBtn('Raw', false);
  tabs.append(previewTab, rawTab);

  const close = document.createElement('button');
  close.type = 'button';
  close.className = 'pi-artifact-close';
  close.textContent = '✕';
  close.setAttribute('aria-label', 'Close artifact viewer');
  head.append(title, type, tabs, close);

  // The server's render, isolated: scripts may run inside the artifact's own
  // document, but it gets no same-origin reach back into the page.
  const frame = document.createElement('iframe');
  frame.className = 'pi-artifact-frame';
  frame.setAttribute('sandbox', 'allow-scripts');
  frame.title = 'Rendered artifact';
  frame.src = el._endpoint + '/artifacts/' + encodeURIComponent(meta.artifact_id)
    + '/ui?token=' + encodeURIComponent(el._token);

  const raw = document.createElement('pre');
  raw.className = 'pi-artifact-raw';
  raw.hidden = true;

  const show = (preview) => {
    frame.hidden = !preview;
    raw.hidden = preview;
    previewTab.setAttribute('aria-selected', String(preview));
    rawTab.setAttribute('aria-selected', String(!preview));
    if (!preview && !raw.textContent) {
      raw.textContent = 'loading…';
      void getJson(artifactUrl(el, meta.artifact_id)).then((body) => {
        raw.textContent = body ? pretty(body.data == null ? body : body.data)
          : 'could not load this artifact';
      });
    }
  };
  previewTab.addEventListener('click', () => show(true));
  rawTab.addEventListener('click', () => show(false));

  overlay.append(head, frame, raw);
  el._artifactOverlay = overlay;

  close.addEventListener('click', () => closeArtifact(el));
  el._onArtifactKey = (e) => {
    if (e.key === 'Escape') closeArtifact(el);
  };
  document.addEventListener('keydown', el._onArtifactKey);

  // Inside the terminal so it scopes to the pane, positioned by CSS over the
  // transcript; the composer below stays visible and typeable.
  el.querySelector('.pi-body-wrap').append(overlay);
  close.focus();
}

export function closeArtifact(el) {
  if (el._onArtifactKey) {
    document.removeEventListener('keydown', el._onArtifactKey);
    el._onArtifactKey = null;
  }
  if (el._artifactOverlay) {
    el._artifactOverlay.remove();
    el._artifactOverlay = null;
  }
}

function tabBtn(label, selected) {
  const btn = document.createElement('button');
  btn.type = 'button';
  btn.className = 'pi-artifact-tab';
  btn.textContent = label;
  btn.setAttribute('role', 'tab');
  btn.setAttribute('aria-selected', String(selected));
  return btn;
}
