/**
 * The two whole-pane shells <sp-auth-pane> swaps between — the anonymous
 * sign-in/registration form, and the signed-in profile header. Pure strings;
 * the controller does the wiring.
 */

import {
  tabsHtml, panelHtml, overviewHtml, trafficHtml, usageHtml, governanceHtml,
} from './sp-auth-pane-view.js';

const TEAM_SIZES = ['1–10', '11–50', '51–200', '201–1000', '1000+'];

export function signinFormHtml() {
  return ''
    + '<form class="pane-form" data-role="signin">'
    + '<label class="pane-label" for="ap-signin-email">Email</label>'
    + '<input class="pane-field" id="ap-signin-email" type="email" autocomplete="email"'
    + ' placeholder="you@company.com" required>'
    + '<button type="submit" class="pane-btn pane-btn--primary">Continue with passkey</button>'
    + '<p class="pane-note">No password — your device authenticates you.</p>'
    + '</form>';
}

/**
 * Two steps rather than one long column: the profile fields are what a human
 * reads when approving the account, so they are not optional, but asking for
 * six of them before anything has happened reads as a wall.
 */
export function registerFormHtml() {
  const sizes = TEAM_SIZES.map((s) => '<option>' + s + '</option>').join('');
  return ''
    + '<form class="pane-form" data-role="register" hidden>'
    + '<fieldset class="pane-step" data-role="step-1">'
    + '<label class="pane-label" for="ap-reg-email">Work email</label>'
    + '<input class="pane-field" id="ap-reg-email" type="email" autocomplete="email"'
    + ' placeholder="you@company.com" required>'
    + '<label class="pane-label" for="ap-reg-name">Your name</label>'
    + '<input class="pane-field" id="ap-reg-name" type="text" autocomplete="name" required>'
    + '<button type="button" class="pane-btn pane-btn--primary" data-role="next">Continue</button>'
    + '</fieldset>'
    + '<fieldset class="pane-step" data-role="step-2" hidden>'
    + '<label class="pane-label" for="ap-reg-company">Company</label>'
    + '<input class="pane-field" id="ap-reg-company" type="text" autocomplete="organization" required>'
    + '<label class="pane-label" for="ap-reg-role">Role</label>'
    + '<input class="pane-field" id="ap-reg-role" type="text" autocomplete="organization-title" required>'
    + '<label class="pane-label" for="ap-reg-team">Engineers using AI tools</label>'
    + '<select class="pane-field" id="ap-reg-team" required>' + sizes + '</select>'
    + '<label class="pane-label" for="ap-reg-why">What are you evaluating?</label>'
    + '<textarea class="pane-field" id="ap-reg-why" rows="3" required'
    + ' placeholder="Governing Claude Code across the team"></textarea>'
    + '<div class="pane-actions">'
    + '<button type="button" class="pane-btn pane-btn--ghost" data-role="back">Back</button>'
    + '<button type="submit" class="pane-btn pane-btn--primary">Create account</button>'
    + '</div>'
    + '<p class="pane-note">The terminal, your $5 credit, and the Bridge are '
    + 'yours the moment you register.</p>'
    + '</fieldset>'
    + '</form>';
}

export function authHtml() {
  return ''
    + '<div class="pane">'
    // The offer sits above the form rather than inside the copy under it.
    // It is the reason to complete the form, and a visitor who reads only one
    // element on this half of the page should read this one.
    + '<div class="pane-offer">'
    + '<strong class="pane-offer-amount">$5 of free AI</strong>'
    + '<span class="pane-offer-line">on us, to learn what systemprompt.io does</span>'
    + '<span class="pane-offer-fine">No card. Passkey only. Spend it in the terminal '
    + 'on the left and watch every cent land in your own audit trail.</span>'
    + '</div>'
    + '<header class="pane-head">'
    + '<h2 class="pane-title">Drive it yourself</h2>'
    + '<p class="pane-sub">The terminal on the left is a replay until you sign in. '
    + 'With an account it runs a real agent whose every tool call stops for your '
    + 'approval — and everything it does lands here.</p>'
    + '</header>'
    + '<div class="pane-tabs" role="tablist">'
    + '<button type="button" class="pane-tab is-active" data-role="tab-signin" role="tab">Sign in</button>'
    + '<button type="button" class="pane-tab" data-role="tab-register" role="tab">Create account</button>'
    + '</div>'
    + '<div class="pane-alert" data-role="alert" hidden></div>'
    + '<div class="pane-busy" data-role="busy" hidden><span class="pane-spinner"></span>'
    + '<span data-role="busy-text"></span></div>'
    + signinFormHtml()
    + registerFormHtml()
    // Lifetime totals only, and only once they arrive. A visitor who has not
    // signed in is shown that the deployment has governed real traffic — the
    // one claim worth making before they have any numbers of their own — and
    // nothing narrow enough to be about a person.
    + '<section class="pane-section pane-section--pulse" data-role="pulse" hidden>'
    + '<h3 class="pane-h3">Across the platform '
    + '<span class="pane-h3-sub" data-role="pulse-window"></span></h3>'
    + '<p class="pane-pulse-note" data-role="pulse-all-time"></p>'
    + '</section>'
    + '</div>';
}

export function profileHtml(pending) {
  return ''
    + '<div class="pane">'
    + '<header class="pane-head pane-head--profile">'
    + '<div class="pane-id">'
    + '<span class="pane-avatar" data-role="avatar"></span>'
    + '<div><strong class="pane-name" data-role="name"></strong>'
    + '<span class="pane-email" data-role="email"></span></div>'
    + '</div>'
    + '<span class="pane-badge" data-role="badge"></span>'
    + '</header>'
    + (pending
      ? '<p class="pane-note pane-note--pending">Your account is under review. '
        + 'The terminal is yours now; the $5 credit and the Bridge unlock once a '
        + 'human approves it.</p>'
      : '')
    // The credit meter sits above the tabs, not inside one: it is the one
    // number that must stay visible whatever the visitor is looking at.
    + '<section class="pane-section pane-section--credit" data-role="credit" hidden>'
    + '<h3 class="pane-h3">Your credit <span class="pane-h3-sub" data-role="credit-of"></span></h3>'
    + '<div class="pane-credit">'
    + '<div class="pane-credit-figure">'
    + '<strong class="pane-credit-left" data-role="credit-left">$0</strong>'
    + '<span class="pane-credit-cap">left</span>'
    + '</div>'
    // A meter rather than a bare number: the shape of the bar is what makes
    // "you have barely touched it" legible at a glance, which is the whole
    // point of showing a free grant back to the person who was given it.
    + '<div class="pane-credit-bar" data-role="credit-bar" role="img"'
    + ' aria-label="credit remaining"><span data-role="credit-fill"></span></div>'
    + '<p class="pane-credit-note" data-role="credit-note"></p>'
    + '</div>'
    + '</section>'
    // Four tabs, each one question: what is happening, how much and how
    // fast, what it consumed, and what policy did about it. Every panel is
    // rendered once and stays in the DOM; switching only toggles `hidden`,
    // so live updates keep landing in panels the visitor is not looking at.
    + tabsHtml()
    + panelHtml('overview', overviewHtml(), false)
    + panelHtml('traffic', trafficHtml(), true)
    + panelHtml('usage', usageHtml(), true)
    + panelHtml('governance', governanceHtml(), true)
    // Hidden until the pulse arrives. A visitor's own numbers prove we record
    // them; this proves the machinery is not a diorama built for one person.
    + '<section class="pane-section pane-section--pulse" data-role="pulse" hidden>'
    + '<h3 class="pane-h3">Across the platform '
    + '<span class="pane-h3-sub" data-role="pulse-window"></span></h3>'
    + '<dl class="pane-stats pane-stats--pulse" data-role="pulse-stats"></dl>'
    + '<p class="pane-pulse-note" data-role="pulse-models"></p>'
    + '<p class="pane-pulse-note" data-role="pulse-all-time"></p>'
    + '</section>'
    + '<footer class="pane-foot">'
    + '<button type="button" class="pane-link" data-role="signout">Sign out</button>'
    + '</footer>'
    + '</div>';
}
