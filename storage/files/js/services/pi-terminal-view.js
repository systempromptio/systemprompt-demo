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
  // The conversation picker lives in the header rather than above the shell:
  // it is terminal chrome, and as a sibling it broke the pane's geometry. The
  // chip toggles a dropdown that hosts <sp-conversation-list> — the terminal
  // instantiates it in _build(), the list keeps owning its own behaviour.
  + '<span class="pi-conv" data-role="conv-wrap">'
  + '<button type="button" class="pi-conv-chip" data-role="conv-chip"'
  + ' aria-haspopup="true" aria-expanded="false"'
  + ' title="Your conversations">Conversations</button>'
  + '<div class="pi-conv-panel" data-role="conv-panel" hidden></div>'
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
  + '<a class="pi-trace-link" data-role="trace" href="/admin/demo/trace" hidden>audit trail →</a>'
  + '</div>'
  + '<div class="pi-body-wrap">'
  + '<div class="terminal-body" data-role="body" tabindex="0" role="log"'
  + ' aria-live="polite" aria-relevant="additions" aria-label="Agent transcript"></div>'
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
  + ' placeholder="Ask the agent something, or type / for skills…"'
  + ' role="combobox" aria-expanded="false" aria-controls="pi-palette-list"'
  + ' aria-autocomplete="list" disabled></textarea>'
  // Conversation controls, quietest to loudest: Clear (ghost, session up),
  // Stop (mid-turn only), Send (primary, at the edge where the eye expects
  // the commit action). Send wears the return glyph because that is the key
  // that does the same thing — the button and the hint state one contract.
  + '<button type="button" class="pi-btn pi-btn--ghost pi-btn--clear" data-role="clear" hidden'
  + ' title="Clear this conversation and start a new one">'
  + '<svg class="pi-btn__icon" viewBox="0 0 12 12" aria-hidden="true" focusable="false">'
  + '<path d="M2.5 4.5a4 4 0 1 1-.4 3" fill="none" stroke="currentColor"'
  + ' stroke-width="1.4" stroke-linecap="round"/>'
  + '<path d="M2.1 2.2v2.5h2.5" fill="none" stroke="currentColor"'
  + ' stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"/>'
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
