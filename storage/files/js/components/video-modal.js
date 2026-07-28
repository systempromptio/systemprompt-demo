// YouTube facade: no iframe exists until a `[data-video-id]` element is
// clicked, so an unwatched demo costs the homepage nothing.
// Optional attributes: data-video-start (seconds), data-video-title,
// data-video-source.

let dialog = null;
let frameWrap = null;
let titleEl = null;
let lastTrigger = null;

const closeModal = () => {
  if (!dialog) return;
  if (typeof dialog.close === 'function') dialog.close();
  else dialog.removeAttribute('open');
};

const cleanup = () => {
  if (frameWrap) frameWrap.innerHTML = '';
  if (lastTrigger && typeof lastTrigger.focus === 'function') lastTrigger.focus();
};

const buildDialog = () => {
  dialog = document.createElement('dialog');
  dialog.className = 'video-modal';
  dialog.innerHTML = `
    <div class="video-modal__inner">
      <div class="video-modal__header">
        <h2 class="video-modal__title" id="video-modal-title"></h2>
        <button type="button" class="video-modal__close" aria-label="Close video">
          <svg aria-hidden="true" width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/>
          </svg>
        </button>
      </div>
      <div class="video-modal__frame" data-frame></div>
    </div>
  `;
  dialog.setAttribute('aria-labelledby', 'video-modal-title');
  document.body.appendChild(dialog);
  frameWrap = dialog.querySelector('[data-frame]');
  titleEl = dialog.querySelector('.video-modal__title');

  dialog.querySelector('.video-modal__close').addEventListener('click', closeModal);
  dialog.addEventListener('click', (event) => {
    const inner = dialog.querySelector('.video-modal__inner');
    if (inner && !inner.contains(event.target)) closeModal();
  });
  dialog.addEventListener('close', cleanup);
};

const openVideo = (videoId, opts) => {
  if (!dialog) buildDialog();
  const start = opts.start ? `&start=${encodeURIComponent(opts.start)}` : '';
  const src = `https://www.youtube-nocookie.com/embed/${encodeURIComponent(videoId)}?autoplay=1&rel=0&modestbranding=1${start}`;
  const title = opts.title || 'Video';
  titleEl.textContent = title;
  frameWrap.innerHTML = `
    <iframe
      src="${src}"
      title="${title.replace(/"/g, '&quot;')}"
      allow="accelerometer; autoplay; clipboard-write; encrypted-media; gyroscope; picture-in-picture; web-share"
      referrerpolicy="strict-origin-when-cross-origin"
      allowfullscreen></iframe>
  `;
  if (typeof dialog.showModal === 'function') dialog.showModal();
  else dialog.setAttribute('open', '');
};

document.addEventListener('click', (event) => {
  const trigger = event.target.closest?.('[data-video-id]');
  if (!trigger) return;
  // The trigger keeps its YouTube href so it still works without JS.
  if (trigger.tagName === 'A') event.preventDefault();
  lastTrigger = trigger;
  openVideo(trigger.getAttribute('data-video-id'), {
    start: trigger.getAttribute('data-video-start'),
    title: trigger.getAttribute('data-video-title'),
    source: trigger.getAttribute('data-video-source'),
  });
});

document.addEventListener('keydown', (event) => {
  if (event.key === 'Escape' && dialog && dialog.hasAttribute('open')) closeModal();
});
