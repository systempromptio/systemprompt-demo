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

/** Stats poll. Matches the interval the pane beside this one already uses. */
export const STATS_MS = 3000;

/** Capacity meter poll. The queued cadence doubles as the waitlist heartbeat —
 *  the server drops a waiter it has not heard from in 30s, so this must stay
 *  well inside that. */
export const CAPACITY_MS = 10000;
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
