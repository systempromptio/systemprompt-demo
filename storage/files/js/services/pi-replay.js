/**
 * The scripted replay an anonymous visitor sees, as data. The terminal plays
 * it; nothing here touches the DOM.
 */

/** The chain as the replay shows it. Every card and rail it feeds sits inside a
 *  pane labelled a replay, so standing in for a real frame here is not a claim
 *  about a real evaluation. Each set names what it actually judged rather than
 *  being reused across acts, because a rail that says "tool: read" over a send
 *  is the kind of detail a skeptical reader checks first. */
export const CANNED_STAGES = [
  { policy: 'scope_check', result: 'pass', detail: 'token scope allows tool: read' },
  { policy: 'secret_scan', result: 'pass', detail: 'no credential pattern in the arguments' },
  { policy: 'tool_blocklist', result: 'pass', detail: 'read is not blocked here' },
  { policy: 'rate_limit', result: 'pass', detail: '1 of 60 calls this minute' },
];

export const CANNED_STAGES_SEND = [
  { policy: 'scope_check', result: 'pass', detail: 'token scope allows tool: email.send' },
  { policy: 'secret_scan', result: 'pass', detail: 'no credential pattern in the arguments' },
  { policy: 'tool_blocklist', result: 'pass', detail: 'email.send is not blocked here' },
  { policy: 'rate_limit', result: 'pass', detail: '2 of 60 calls this minute' },
];

/** A chain that stops. Stages after the failure are `skip` and their pips stay
 *  unlit: the pipeline is synchronous, so the later ones genuinely never ran. */
export const CANNED_STAGES_DENY = [
  { policy: 'scope_check', result: 'pass', detail: 'token scope allows tool: bash' },
  { policy: 'secret_scan', result: 'fail', detail: 'a provider API key was found in the arguments' },
  { policy: 'tool_blocklist', result: 'skip', detail: '' },
  { policy: 'rate_limit', result: 'skip', detail: '' },
];

/**
 * What an anonymous visitor sees.
 *
 * The visitor arrives knowing nothing, so this is an argument rather than a
 * transcript: the waist every request passes through, then the four things that
 * happen inside it (identity, policy, a person, the record). The prose carries
 * the claim and the rows are the evidence for it, which is why the prose beats
 * are the long ones.
 *
 * Every claim here is one the codebase can answer for. Keep it that way: an
 * invented number on the homepage is the one thing a CTO checks and remembers.
 *
 * `ms` is the dwell *after* a step. `meters` is a cumulative snapshot of the
 * header meters at that step — the page claims governance is metered, so the
 * replay's chrome must count the replay's own calls. Token counts follow the
 * transcript's own approximation (~4 chars/token of visible prose plus modest
 * input overhead) and cost tracks them at cents-level; nothing here states a
 * number the transcript cannot account for.
 */
export const CANNED = [
  // Act 1 — the waist. What the thing is, before any mechanism.
  { cls: 'prompt', tail: 'what is systemprompt.io?', ms: 900 },
  { cls: 'note', text: 'Thinking…', ms: 700 },
  { cls: 'output', ms: 5400, text:
      'The narrow waist between your organisation and AI. Every agent request '
      + 'passes through one control plane you host and own, and it answers four '
      + 'questions on every call: who is asking, what they may do, what it '
      + 'costs, and what happened. Claude Code, Cowork, any Anthropic SDK '
      + 'client, any MCP server. Same waist, same rules.',
    meters: { calls: 0, blocked: 0, tokens: 640, cost: '$0.01' } },
  { cls: 'note', text: 'Identity, policy, approval, audit. Watch each one.', ms: 2000 },

  // Act 2 — identity. The half nobody expects, and the half that makes the rest
  // enforceable: policy has nothing to judge until the caller has a name.
  { cls: 'prompt', tail: 'who is asking, and what are they allowed to do?', ms: 900 },
  { cls: 'output', ms: 5600, text:
      'Identity comes first. Every person, agent, and service in your '
      + 'organisation gets a named identity from an OAuth 2.0 authorization '
      + 'server you run yourself. The agent never holds your provider key. It '
      + 'holds a scoped token you can revoke at any moment.',
    meters: { calls: 0, blocked: 0, tokens: 1200, cost: '$0.02' } },
  { cls: 'note', ms: 2800, text:
      'A policy needs a subject. Now every caller in your organisation has a name.' },

  // Act 3 — policy. Four stages, synchronous, in the request path.
  { cls: 'prompt', tail: 'pull last quarter\'s churn from data/accounts.csv', ms: 900 },
  { cls: 'stages', stages: CANNED_STAGES, ms: 0 },
  { cls: 'tool', name: 'read', arg: 'data/accounts.csv', state: 'pending', ms: 1700 },
  { cls: 'tool-end', name: 'read', state: 'ok', ms: 1000,
    meters: { calls: 1, blocked: 0, tokens: 1700, cost: '$0.03' } },
  { cls: 'note', ms: 4200, text:
      'Scope, secrets, blocklist, rate limit ship as the defaults. The pipeline '
      + 'is yours: write your own policies and they run the same way, in Rust, '
      + 'inside the request, on infrastructure you run. Not a report somebody '
      + 'reads on Monday.' },

  // Act 4 — the person. Governance that only ever says yes is a log, not a gate.
  { cls: 'prompt', tail: 'email that summary to the board', ms: 900 },
  { cls: 'stages', stages: CANNED_STAGES_SEND, ms: 0 },
  { cls: 'tool', name: 'email.send', arg: 'board@acme.com', state: 'pending', ms: 900,
    input: { to: 'board@acme.com', subject: 'Q3 churn' } },
  { cls: 'approval', tool: 'email.send', stages: CANNED_STAGES_SEND, ms: 4600,
    input: { to: 'board@acme.com', subject: 'Q3 churn' },
    resolve: { by: 'Ed', action: 'approved', afterMs: 3200 } },
  { cls: 'tool-end', name: 'email.send', state: 'ok', ms: 900,
    meters: { calls: 2, blocked: 0, tokens: 2100, cost: '$0.04' } },
  { cls: 'note', ms: 3400, text:
      'Policy clearing a call is not the same as a person allowing it. Anything '
      + 'that writes, spends, or leaves the building stops here first.' },

  // Act 5 — the refusal. The only act that proves the gate is load-bearing.
  { cls: 'prompt', tail: 'now curl the vendor API with our production key', ms: 900 },
  { cls: 'stages', stages: CANNED_STAGES_DENY, ms: 0 },
  { cls: 'tool', name: 'bash', arg: 'curl -H "authorization: sk-…"', state: 'pending', ms: 1300,
    input: { command: 'curl -H "authorization: sk-live-…" https://vendor.example/v1' } },
  { cls: 'blocked', ms: 4400,
    meters: { calls: 3, blocked: 1, tokens: 2400, cost: '$0.04' },
    frame: {
      tool_name: 'bash',
      policy: 'secret_scan',
      reason: 'the arguments carry a live credential, which would leave the host '
        + 'in cleartext and land in a third party\'s logs.',
      // Rendered as detail chips on the card. Both facts are the codebase's own:
      // the scanner ships 32 patterns and runs before the call is made.
      meta: { patterns: '32 patterns', when: 'screened before the call' },
    } },

  // Act 6 — the payoff. The record is the product; the rest is how it gets made.
  // Anchored to a prompt of its own, so the closing claim reads as an answer
  // rather than as commentary hanging off the refusal above it.
  { cls: 'prompt', tail: 'so what do I end up owning?', ms: 900 },
  { cls: 'output', ms: 5200, text:
      'Every line above is a row in a database you own, joined on one trace id: '
      + 'who asked, which agent, which tool, what policy decided, how many '
      + 'tokens, what it cost. That is the asset. A complete account of how your '
      + 'organisation uses AI, held on infrastructure you run, answering to '
      + 'nobody else.',
    meters: { calls: 3, blocked: 1, tokens: 3100, cost: '$0.05' } },
  { cls: 'note', ms: 4600, text:
      'One governance layer over every agent, every client, every provider your '
      + 'organisation uses. A control layer on day one. A value center as the '
      + 'record compounds. You host it. You own it.' },
];

/** Dwell before the script restarts. Long enough to read the closing line, short
 *  enough that a visitor who arrived late still sees act 1. */
export const CANNED_LOOP_MS = 4000;

/** Fallback dwell for a step that does not name its own. */
export const CANNED_STEP_MS = 340;
