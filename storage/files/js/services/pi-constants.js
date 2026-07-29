/**
 * The one place the pi surface's endpoint and tuning constants live.
 * Every pi component imports from here; no literal '/api/public/pi'
 * appears anywhere else in JS.
 */

export const PI_API_BASE = '/api/public/pi';

/** Frames the widget renders as a tool row, keyed by tool_use_id. */
export const TOOL_ICON = { pending: '▸', ok: '●', blocked: '✗' };

/** Backoff for manual stream reconnects. EventSource's own retry gives up. */
export const RECONNECT_MIN_MS = 1000;
export const RECONNECT_MAX_MS = 30000;

/** How close to the bottom still counts as "following the output". Roughly one
 *  line of slack, so a trackpad's inertial overscroll does not unpin the view. */
export const PIN_SLACK_PX = 32;

/** Transcript cap, and how much is dropped when it is hit. Trimming in batches
 *  keeps the reflow cost off every line once a long session gets there. */
export const MAX_LINES = 1200;
export const TRIM_BATCH = 200;

/** Waitlist heartbeat. Deliberately a poll: a queued visitor has no
 *  conversation, so there is no SSE stream to push a position to them — and
 *  the endpoint is an in-memory registry read. The server drops a waiter it
 *  has not heard from in 30s, so this must stay well inside it. */
export const CAPACITY_QUEUE_MS = 5000;

/** Prompts kept for ↑/↓ recall. In memory only — a governed transcript is not
 *  something to leave in localStorage on a shared machine. */
export const HISTORY_MAX = 50;

/** Composer ceiling, in rows. Past this the transcript matters more than seeing
 *  the whole draft. */
export const INPUT_MAX_ROWS = 6;
export const INPUT_ROW_PX = 22;

/** History pages fetched when restoring. The server caps each page, so this
 *  bounds a restore at a few round trips rather than an unbounded loop against
 *  a conversation that keeps reporting more. */
export const REPLAY_PAGES = 8;

/** Why a session ended, keyed by the `reason` on the exit frame. The server
 *  knows which of four evictions fired; without this the widget could only say
 *  "ended", and a visitor evicted by their own second tab would read it as a
 *  crash. Unknown or absent — a frame replayed from before the field existed —
 *  falls through to no explanation rather than a guess. */
export const EXIT_REASONS = {
  idle: 'idle for too long',
  max_lifetime: 'demo sessions are capped at an hour',
  child_exited: 'the agent process stopped',
  superseded: 'you started a session in another tab',
  resumed: 'it was reopened elsewhere',
  closed: 'you closed this conversation',
};
