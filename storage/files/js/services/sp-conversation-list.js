import { PI_API_BASE } from './pi-constants.js';
import { conversations, sendJson } from './pi-transport.js';

/**
 * <sp-conversation-list for="pi" endpoint="/api/public/pi">
 *
 * The picker in the terminal's header dropdown (the terminal instantiates it
 * inside .pi-conv-panel): every conversation this viewer owns, a button
 * to start a new one, and rename/delete on each. Light DOM, like the terminal,
 * so the global --sp-* tokens and [data-theme] apply.
 *
 * It holds no conversation state of its own. The list comes from Postgres on
 * every refresh, and switching conversations is a call into the terminal — which
 * owns the session, the stream, and the credential. Two components that both
 * believed they knew the current conversation would eventually disagree.
 */

/** Longest title shown before it is ellipsised in CSS rather than in the DOM,
 *  so the full text stays available as a tooltip. */
const TITLE_FALLBACK = 'Untitled conversation';

class SpConversationList extends HTMLElement {
  constructor() {
    super();
    this._token = null;
    this._current = null;
    this._items = [];
  }

  connectedCallback() {
    if (this._built) return;
    this._built = true;
    this._endpoint = (this.getAttribute('endpoint') || PI_API_BASE).replace(/\/$/, '');
    this._build();

    // The terminal is the source of truth for both the credential and which
    // conversation is open, and it announces both. Listening on the document
    // rather than on a resolved element keeps this working wherever the two
    // sit relative to each other in the page.
    document.addEventListener('pi-session', (e) => {
      this._token = e.detail && e.detail.token;
      this._current = e.detail && e.detail.conversation_id;
      void this.refresh();
    });
    document.addEventListener('pi-conversations-changed', () => void this.refresh());
    document.addEventListener('sp-auth:signed-out', () => {
      this._token = null;
      this._current = null;
      this._render([]);
    });
  }

  /** The terminal this drives, resolved late so markup order does not matter. */
  get _terminal() {
    const id = this.getAttribute('for');
    return (id && document.getElementById(id)) || document.querySelector('sp-pi-terminal');
  }

  _build() {
    this.replaceChildren();
    const head = document.createElement('div');
    head.className = 'conv-head';

    const label = document.createElement('h2');
    label.className = 'conv-title';
    label.textContent = 'Conversations';

    this._newBtn = document.createElement('button');
    this._newBtn.type = 'button';
    this._newBtn.className = 'conv-new';
    this._newBtn.textContent = 'New';
    this._newBtn.addEventListener('click', () => void this._new());

    head.append(label, this._newBtn);

    this._listEl = document.createElement('ul');
    this._listEl.className = 'conv-list';

    this._emptyEl = document.createElement('p');
    this._emptyEl.className = 'conv-empty';
    this._emptyEl.textContent = 'Sign in to keep your conversations.';

    this.append(head, this._listEl, this._emptyEl);
  }

  /** Reload from the server. The list is never assembled from local guesses. */
  async refresh() {
    this._render(await conversations(this._endpoint, this._token));
  }

  _render(items) {
    this._items = items;
    this._listEl.replaceChildren();
    this._emptyEl.hidden = items.length > 0;
    if (!items.length) {
      this._emptyEl.textContent = this._token
        ? 'No conversations yet. Ask the agent something.'
        : 'Sign in to keep your conversations.';
      return;
    }
    items.forEach((item) => this._listEl.append(this._row(item)));
  }

  _row(item) {
    const li = document.createElement('li');
    li.className = 'conv-item';
    if (item.id === this._current) li.dataset.current = 'true';

    const open = document.createElement('button');
    open.type = 'button';
    open.className = 'conv-open';
    open.textContent = item.title || TITLE_FALLBACK;
    open.title = item.title || TITLE_FALLBACK;
    if (item.id === this._current) open.setAttribute('aria-current', 'true');
    open.addEventListener('click', () => void this._open(item.id));

    const when = document.createElement('time');
    when.className = 'conv-when';
    when.dateTime = item.updated_at;
    when.textContent = relative(item.updated_at);

    const rename = document.createElement('button');
    rename.type = 'button';
    rename.className = 'conv-action';
    rename.textContent = 'Rename';
    rename.addEventListener('click', () => void this._rename(item));

    const remove = document.createElement('button');
    remove.type = 'button';
    remove.className = 'conv-action conv-action-danger';
    remove.textContent = 'Delete';
    remove.addEventListener('click', () => void this._delete(item));

    const actions = document.createElement('div');
    actions.className = 'conv-actions';
    actions.append(when, rename, remove);

    li.append(open, actions);
    return li;
  }

  async _new() {
    const terminal = this._terminal;
    if (terminal && terminal.newConversation) await terminal.newConversation();
  }

  async _open(id) {
    if (id === this._current) return;
    const terminal = this._terminal;
    if (terminal && terminal.openConversation) await terminal.openConversation(id);
  }

  async _rename(item) {
    const title = window.prompt('Name this conversation', item.title || '');
    // Cancel gives null; an emptied box is a request the server rejects, so it
    // is treated the same as cancelling rather than sent to be refused.
    if (!title || !title.trim()) return;
    await this._call('PATCH', item.id, { token: this._token, title: title.trim() });
  }

  async _delete(item) {
    const name = item.title || TITLE_FALLBACK;
    if (!window.confirm('Delete "' + name + '"? Its transcript stops being shown here.')) return;
    await this._call('DELETE', item.id, { token: this._token });
    // Deleting the open conversation leaves the terminal attached to a child
    // that was just killed, so it has to move somewhere: the newest survivor,
    // or a fresh conversation when there is none.
    if (item.id === this._current) {
      const terminal = this._terminal;
      if (terminal && terminal.newConversation) await terminal.newConversation();
    }
  }

  async _call(method, id, payload) {
    // A failed call is not reported here: the refresh below is what tells the
    // viewer whether it took.
    await sendJson(method, this._endpoint + '/conversations/' + encodeURIComponent(id), payload)
      .catch(() => null);
    await this.refresh();
  }
}

/**
 * A coarse "when", because the exact timestamp is not what a picker is for.
 *
 * Falls back to the raw value rather than to an empty cell: an unparseable date
 * is a bug worth seeing, not one worth hiding.
 */
function relative(iso) {
  const then = Date.parse(iso);
  if (Number.isNaN(then)) return iso || '';
  const mins = Math.max(0, Math.round((Date.now() - then) / 60000));
  if (mins < 1) return 'just now';
  if (mins < 60) return mins + 'm ago';
  const hours = Math.round(mins / 60);
  if (hours < 24) return hours + 'h ago';
  return Math.round(hours / 24) + 'd ago';
}

customElements.define('sp-conversation-list', SpConversationList);
