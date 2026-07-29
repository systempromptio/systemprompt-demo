/**
 * The terminal's chrome, built once at module scope and cloned per instance.
 * Light DOM on purpose — the global --sp-* tokens and [data-theme] apply — so
 * this exports a template singleton rather than a shadow root.
 */

const chrome = document.createElement('template');
chrome.innerHTML = ''
  + '<div class="terminal active">'
  + '<div class="terminal-header">'
  + '<span class="pi-brand">'
  // The same wordmark asset the site header wears (its over-video variant —
  // this chrome is always dark), not a mark-plus-HTML-text reconstruction
  // that could drift from the brand.
  + '<img class="pi-brand-wordmark" src="/files/images/logo-white.svg"'
  + ' alt="systemprompt.io" width="140" height="16" decoding="async">'
  + '</span>'
  + '<span class="pi-live" data-role="live"><i class="pi-live-dot" aria-hidden="true"></i>'
  + '<span class="pi-status" data-role="status"></span></span>'
  + '<span class="pi-jail-chip" data-role="jail" hidden></span>'
  // Who this session is signed to. The padlock is the claim: every request
  // this terminal makes carries a token minted for exactly this identity, so
  // the badge only exists once a session is actually established.
  + '<span class="pi-user-chip" data-role="user" hidden>'
  + '<svg class="pi-user-lock" viewBox="0 0 12 12" aria-hidden="true" focusable="false">'
  + '<rect x="2.25" y="5.25" width="7.5" height="5" rx="1.25" fill="currentColor"/>'
  + '<path d="M4 5V3.75a2 2 0 0 1 4 0V5" fill="none" stroke="currentColor"'
  + ' stroke-width="1.4" stroke-linecap="round"/>'
  + '</svg>'
  + '<span data-role="user-name"></span>'
  + '</span>'
  // Every structured result this conversation's tools produced. Hidden until
  // the first artifact exists — an empty gallery is a promise, not a fact.
  + '<span class="pi-art" data-role="art-wrap" hidden>'
  + '<button type="button" class="pi-art-chip" data-role="art-chip"'
  + ' aria-haspopup="true" aria-expanded="false"'
  + ' title="Structured results this conversation produced">'
  + 'Artifacts <b class="pi-art-count" data-role="art-count">0</b></button>'
  + '<div class="pi-art-panel" data-role="art-panel" hidden></div>'
  + '</span>'
  // Hidden until the catalogue arrives with more than one entry; a picker
  // with one option is furniture.
  + '<select class="pi-model" data-role="model" aria-label="Model" hidden></select>'
  + '<div class="pi-meters" data-role="meters" hidden>'
  + '<span class="pi-meter" data-role="m-tools" title="Tool calls the gate has judged">'
  + '<b>0</b><i>calls</i></span>'
  + '<span class="pi-meter" data-role="m-blocked" title="Calls policy or a person refused">'
  + '<b>0</b><i>blocked</i></span>'
  + '<span class="pi-meter" data-role="m-tokens" title="Tokens this conversation has spent">'
  + '<b>0</b><i>tokens</i></span>'
  + '<span class="pi-meter" data-role="m-cost" title="Metered spend for this conversation">'
  + '<b>$0.00</b><i>cost</i></span>'
  + '</div>'
  + '<a class="pi-trace-link" data-role="trace" href="#" target="_blank" rel="noopener" hidden>audit trail →</a>'
  + '<button type="button" class="pi-expand-chip" data-role="expand" aria-pressed="false"'
  + ' title="Expand the terminal to fill the page">'
  + '<svg viewBox="0 0 12 12" aria-hidden="true" focusable="false">'
  + '<path d="M7.5 1.5h3v3M4.5 10.5h-3v-3M10.5 1.5 7 5M1.5 10.5 5 7" fill="none"'
  + ' stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"/>'
  + '</svg>'
  + '<span data-role="expand-label">Expand</span>'
  + '</button>'
  + '</div>'
  + '<div class="pi-body-wrap">'
  + '<div class="terminal-body" data-role="body" tabindex="0" role="log"'
  + ' aria-live="polite" aria-relevant="additions" aria-label="Agent transcript">'
  + '</div>'
  + '<button type="button" class="pi-jump" data-role="jump" hidden></button>'
  + '</div>'
  + '<div class="pi-approvals" data-role="approvals"></div>'
  + '<div class="pi-palette" data-role="palette" role="listbox"'
  + ' aria-label="Skills" hidden></div>'
  + '<form class="pi-composer" data-role="composer">'
  // Drawn rather than typed: the ASCII `>` inherited the transcript's
  // metrics and could not be made to share a baseline with the input.
  + '<span class="prompt pi-prompt" aria-hidden="true">'
  + '<svg viewBox="0 0 12 12" focusable="false">'
  + '<path d="M3.75 2.5 7.25 6l-3.5 3.5" fill="none" stroke="currentColor"'
  + ' stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round"/>'
  + '</svg></span>'
  + '<label class="sp-sr-only" for="pi-input-field">Ask the agent</label>'
  + '<textarea class="pi-input" id="pi-input-field" data-role="input" rows="1"'
  + ' autocomplete="off" spellcheck="false"'
  + ' placeholder="Try &quot;show me governance in action&quot; — or type / for every demo…"'
  + ' role="combobox" aria-expanded="false" aria-controls="pi-palette-list"'
  + ' aria-autocomplete="list" disabled></textarea>'
  // Composer controls, quietest to loudest: Clear, Stop (mid-turn only), Send
  // (primary, at the edge where the eye expects the commit action). Send
  // wears the return glyph because that is the key that does the same thing
  // — the button and the hint state one contract.
  + '<button type="button" class="pi-btn pi-btn--ghost pi-btn--clear" data-role="clear"'
  + ' title="Clear — start a new conversation">'
  + '<svg class="pi-btn__icon" viewBox="0 0 12 12" aria-hidden="true" focusable="false">'
  + '<path d="M2.5 3.5h7M4.75 3.5V2.25h2.5V3.5M3.5 3.5l.5 6h4l.5-6" fill="none"'
  + ' stroke="currentColor" stroke-width="1.3" stroke-linecap="round"'
  + ' stroke-linejoin="round"/>'
  + '</svg>'
  + '<span class="pi-btn__label">Clear</span>'
  + '</button>'
  + '<button type="button" class="pi-btn pi-btn--ghost pi-btn--stop" data-role="stop" hidden>'
  + '<svg class="pi-btn__icon" viewBox="0 0 12 12" aria-hidden="true" focusable="false">'
  + '<rect x="3" y="3" width="6" height="6" rx="1" fill="currentColor"/></svg>'
  + '<span class="pi-btn__label">Stop</span>'
  + '</button>'
  + '<button type="submit" class="pi-btn pi-btn--send" data-role="send" disabled'
  + ' title="Send (Enter)">'
  + '<svg class="pi-btn__icon" viewBox="0 0 12 12" aria-hidden="true" focusable="false">'
  + '<path d="M9.5 2.5v3.25a1.5 1.5 0 0 1-1.5 1.5H3" fill="none" stroke="currentColor"'
  + ' stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/>'
  + '<path d="M5 4.75 2.5 7.25 5 9.75" fill="none" stroke="currentColor"'
  + ' stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/>'
  + '</svg>'
  + '<span class="pi-btn__label">Send</span>'
  + '</button>'
  + '</form>'
  + '<div class="pi-hint">'
  + '<span class="pi-hint__item"><kbd>↵</kbd>send</span>'
  + '<span class="pi-hint__item"><kbd>⇧↵</kbd>newline</span>'
  + '<span class="pi-hint__item"><kbd>↑↓</kbd>skills</span>'
  + '<span class="pi-hint__item"><kbd>↑</kbd>history</span>'
  + '<span class="pi-hint__item"><kbd>esc</kbd>stop</span>'
  + '</div>'
  + '<div class="pi-gate" data-role="gate" hidden></div>'
  + '</div>';

export function terminalChrome() {
  return chrome.content.cloneNode(true);
}

/**
 * The empty transcript is the one moment nobody knows what to type. These
 * chips are the approved path in — each one submits a real prompt — and the
 * block is removed the moment the first transcript line lands.
 *
 * A separate template from the chrome because an empty transcript happens
 * more than once: Clear empties the body and mounts a fresh copy.
 */
const welcome = document.createElement('template');
welcome.innerHTML = ''
  + '<div class="pi-welcome" data-role="welcome">'
  + '<p class="pi-welcome__lead">A live agent behind a governance gateway — every '
  + 'tool call below is checked, audited, and priced in front of you. Try one:</p>'
  + '<div class="pi-welcome__chips">'
  + '<button type="button" class="pi-welcome__chip" data-chip'
  + ' data-prompt="/skill:explain-systemprompt">What is systemprompt.io?</button>'
  + '<button type="button" class="pi-welcome__chip" data-chip'
  + ' data-prompt="/skill:demonstrate-governance">Watch a leaked secret get blocked</button>'
  // No data-prompt: the credential is minted per click so the transcript
  // shows a key nobody could have pre-baked into the page.
  + '<button type="button" class="pi-welcome__chip" data-chip data-secret>'
  + 'Paste a dummy secret and watch it get blocked</button>'
  + '<button type="button" class="pi-welcome__chip" data-chip'
  + ' data-prompt="/skill:governance-dashboard">Build a live dashboard of this session</button>'
  + '<button type="button" class="pi-welcome__chip" data-chip'
  + ' data-prompt="/skill:audit-this-session">Audit this session — what did it cost?</button>'
  + '<button type="button" class="pi-welcome__chip" data-chip'
  + ' data-prompt="What can you do?">What can you do?</button>'
  + '</div>'
  + '<p class="pi-welcome__more">or type <kbd>/</kbd> for the full demo menu</p>'
  + '</div>';

/** Draw the welcome block into an empty transcript and track it for removal. */
export function mountWelcome(el) {
  el._body.append(welcome.content.cloneNode(true));
  el._welcomeEl = el._body.querySelector('[data-role="welcome"]');
}
