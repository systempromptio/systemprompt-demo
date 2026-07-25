/**
 * The enforcement point for a governed pi session. It decides nothing.
 *
 * pi runs its tools in-process, so a proxy watching the event stream from
 * outside cannot stop one: `tool_execution_start` is emitted before this hook
 * resolves, and the only external lever is `abort`, which kills a whole turn.
 * The gate therefore has to live in here — but the *decision* lives in Rust.
 *
 * The channel between them is pi's own `ctx.ui.confirm`. In `--mode rpc` it
 * emits an `extension_ui_request` on stdout and suspends until the client writes
 * an `extension_ui_response` to stdin. The proxy already owns both pipes, so it
 * is the counterparty by construction: no HTTP, no port, no credential in here
 * to leak. `confirm` has no client-side timeout, so the proxy owns the clock.
 *
 * Everything that is not an explicit `true` blocks. There is deliberately no
 * FAIL_OPEN switch: flipping one would turn the gate into advice, and it would
 * be flipped in the one file nobody reviews.
 */

import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";

/** Title the proxy matches on, distinguishing our gate from any other dialog. */
const CHANNEL = "sp-governance";

/**
 * The session the gateway attests. Sent on every provider request so model spend
 * and governance decisions land on one joinable id.
 */
const SESSION_ID = process.env.SYSTEMPROMPT_PI_SESSION ?? "";

type Payload = {
  kind: "prompt" | "tool";
  prompt?: string;
  tool_name?: string;
  tool_use_id?: string;
  tool_input?: unknown;
};

export default function (pi: ExtensionAPI) {
  pi.on("before_provider_headers", (event) => {
    if (SESSION_ID) event.headers["x-session-id"] = SESSION_ID;
    return undefined;
  });

  /**
   * Ask the proxy. A throw means the channel is gone — during shutdown, or if
   * the response is malformed — and an unanswerable request is a denial.
   */
  const permitted = async (
    ctx: { ui: { confirm(title: string, message: string): Promise<boolean> } },
    payload: Payload,
  ): Promise<boolean> => {
    try {
      return await ctx.ui.confirm(CHANNEL, JSON.stringify(payload));
    } catch {
      return false;
    }
  };

  // Prompt gate. Runs before any provider request, so a credential pasted into
  // the box is caught while it is still local to this machine.
  pi.on("input", async (event, ctx) => {
    const ok = await permitted(ctx, { kind: "prompt", prompt: event.text });
    return ok ? { action: "continue" } : { action: "handled" };
  });

  // Tool gate. Returning `block` keeps the call from executing at all; the
  // reason is generic because `confirm` answers a bare boolean, and the specific
  // policy reason is audited and shown in the widget instead.
  pi.on("tool_call", async (event, ctx) => {
    const ok = await permitted(ctx, {
      kind: "tool",
      tool_name: event.toolName,
      tool_use_id: event.toolCallId,
      tool_input: event.input,
    });
    return ok ? undefined : { block: true, reason: "[GOVERNANCE] denied" };
  });
}
