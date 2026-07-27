/**
 * The `systemprompt` documentation hub, as pi tools.
 *
 * pi has no MCP client and says so deliberately — "build an extension that adds
 * MCP support" — so this is that extension, scoped to one hub. It registers one
 * pi tool per hub tool and forwards each call to the proxy endpoint on the
 * gateway, which owns the identity headers the hub trusts.
 *
 * Two things about it are load-bearing:
 *
 * 1. **The tool names are the MCP names, verbatim.** `mcp__systemprompt__*` is
 *    what a skill body writes, what `scope_check` matches its admin-only
 *    prefixes against, and what lands in the audit row. Renaming them here
 *    would mean skills needed one dialect for Claude Desktop and another for
 *    this terminal, and would quietly change what the policy chain sees.
 *
 * 2. **There is no `tool_call` hook in here.** The governance shim is the
 *    single enforcement point, and its hook already fires for extension tools.
 *    A second gate in this file would be a second thing to keep correct, and
 *    the one people would forget when adding a tool.
 *
 * The credentials arrive by environment rather than being embedded: the proxy
 * checks them against the conversation on every call, so a leaked copy is worth
 * only what the conversation itself is worth, and only while it lives.
 */

import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { Type } from "typebox";

const BASE_URL = process.env.SYSTEMPROMPT_BASE_URL ?? "";
const TOKEN = process.env.SP_PI_MCP_TOKEN ?? "";
const CONVERSATION = process.env.SP_PI_CONVERSATION ?? "";

/** Where the proxy lives. One endpoint; the tool travels in the body. */
const ENDPOINT = `${BASE_URL}/api/public/pi/mcp`;

type HubTool = {
  /** Suffix after `mcp__systemprompt__`, and the name the hub knows. */
  readonly name: string;
  readonly label: string;
  readonly description: string;
  readonly parameters: unknown;
};

const TOOLS: readonly HubTool[] = [
  {
    name: "list_topics",
    label: "List Docs",
    description:
      "List every systemprompt.io documentation topic with its id and a one-line " +
      "summary. Start here, then read one with mcp__systemprompt__get_topic.",
    parameters: Type.Object({}),
  },
  {
    name: "get_topic",
    label: "Read Doc",
    description:
      "Return the full Markdown of one documentation topic by its id, as listed " +
      "by mcp__systemprompt__list_topics.",
    parameters: Type.Object({
      topic_id: Type.String({
        description: 'Topic id, e.g. "governance-pipeline".',
      }),
    }),
  },
  {
    name: "search_docs",
    label: "Search Docs",
    description:
      "Keyword search across all documentation topics; returns ranked topics " +
      "with short excerpts.",
    parameters: Type.Object({
      query: Type.String({
        description: 'A question or keywords, e.g. "how are secrets blocked".',
      }),
    }),
  },
  {
    name: "governance_stats",
    label: "Governance Stats",
    description:
      "Return your own governance spine: every policy verdict with its reason, " +
      "provider spend and latency, and which tools actually ran. No arguments — " +
      "the subject is whoever is calling.",
    parameters: Type.Object({}),
  },
  {
    name: "fetch_remote_docs",
    label: "Fetch Upstream Docs",
    description:
      "Fetch a documentation page from the public internet. This deployment " +
      "permits no outbound egress, so the tool_blocklist policy is expected to " +
      "refuse this call before it runs. Call it to demonstrate a refusal; use " +
      "mcp__systemprompt__search_docs for documentation you can actually read.",
    parameters: Type.Object({
      path: Type.String({
        description: 'Path on the public site, e.g. "/docs/governance".',
      }),
    }),
  },
];

export default function (pi: ExtensionAPI) {
  // Without a conversation credential every call would 401. Registering the
  // tools anyway would put five broken affordances in front of the model, so
  // register nothing and let the session run with its built-in tools.
  if (!BASE_URL || !TOKEN || !CONVERSATION) return;

  for (const tool of TOOLS) {
    pi.registerTool({
      name: `mcp__systemprompt__${tool.name}`,
      label: tool.label,
      description: tool.description,
      parameters: tool.parameters,
      async execute(_toolCallId: string, params: unknown, signal: AbortSignal) {
        const text = await callHub(tool.name, params, signal);
        return { content: [{ type: "text", text }], details: {} };
      },
    });
  }
}

/**
 * One call to the proxy.
 *
 * Every failure returns readable text rather than throwing. A thrown error
 * reaches the model as a tool failure with no detail, and the most interesting
 * thing this hub does — being refused — would then be indistinguishable from
 * the hub being down.
 */
async function callHub(
  tool: string,
  params: unknown,
  signal: AbortSignal,
): Promise<string> {
  let response: Response;
  try {
    response = await fetch(ENDPOINT, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        token: TOKEN,
        conversation_id: CONVERSATION,
        tool,
        arguments: params ?? {},
      }),
      signal,
    });
  } catch (e) {
    return `Could not reach the documentation hub: ${e}`;
  }

  if (!response.ok) {
    return `The documentation hub refused the request (HTTP ${response.status}).`;
  }

  try {
    const body = (await response.json()) as { text?: string; ok?: boolean };
    return body.text ?? "The documentation hub returned an empty response.";
  } catch (e) {
    return `Could not read the hub's response: ${e}`;
  }
}
