# Demonstrate Artifact Rendering

Show that this terminal's tools return **typed artifacts**, not walls of text.
Every call below goes through the same governed path as any other tool —
scope check, secret scan, blocklist, rate limit — and lands one artifact on
the shelf (the chip in the terminal header). After each call, point the viewer
at the tool row's **view result** button: the preview it opens is the same
server-side render an MCP host would show, not a second renderer.

## When to Use

Use this when someone asks what the artifact shelf is, how tool results are
rendered, or wants to see each artifact type on screen.

## The Protocol

Follow this loop, mechanically, one artifact per message:

1. Make **one** `render_artifact` call.
2. Wait for its result.
3. Write **one sentence** about the result you just received, and tell the
   viewer to open it from the shelf.
4. Make the next call.

Rules that override everything else in this file:

- **Never make two `render_artifact` calls without narration between them.**
- **Never write a sentence about an artifact whose tool result has not yet
  appeared in this conversation.** The numbered sections below are a menu of
  calls to make, not a script of outcomes to report. If you stopped after
  call 5, you describe 5 artifacts — not 8.
- **Never state a total yourself.** "All eight calls were allowed" is a claim
  only `governance_stats` can make (step 9). If you did not call it, you do
  not know the number.

Claiming a render that did not happen is worse than the failure it papers
over: this skill exists to show the audit spine, and a fabricated line is the
one thing the spine will contradict — the viewer can open the audit trail and
count.

**A denial ends the run.** If any call comes back blocked, stop — do not
continue to the next type. Say which call was refused, quote the reason
verbatim, and then call `governance_stats` to find the deciding policy, because
the reason the model receives is deliberately generic. A block on
`human_approval` means a person declined or the approval lapsed (it times out,
and it is abandoned if the terminal tab has no live viewer); that is the
approval gate doing its job, not the four-stage policy chain refusing the tool.
Say which of the two it was, or say you cannot tell — never guess.

## The Calls

The first three are built from **this session's own governance spine** — the
same rows `governance_stats` reports — so they change as the session does.
The rest carry curated content.

### 1. Table — live audit rows

```
mcp__systemprompt__render_artifact {"artifact_type": "table"}
```

Recent governance decisions for this session: when, which tool, allow or deny,
and the policy that decided. A fresh session gets one illustrative row that
says so.

### 2. Chart — verdicts by policy

```
mcp__systemprompt__render_artifact {"artifact_type": "chart"}
```

The same spine, aggregated: allowed and denied counts per policy stage.

### 3. Dashboard — the session at a glance

```
mcp__systemprompt__render_artifact {"artifact_type": "dashboard"}
```

Verdict totals, provider spend, and the four pipeline stages as status items.

### 4. List — the four governance stages

```
mcp__systemprompt__render_artifact {"artifact_type": "list"}
```

### 5. Presentation card — what systemprompt.io is

```
mcp__systemprompt__render_artifact {"artifact_type": "presentation_card"}
```

### 6. Copy-paste text — a runnable snippet

```
mcp__systemprompt__render_artifact {"artifact_type": "copy_paste_text"}
```

The setup commands, in an artifact built for one-click copying.

### 7. Message — typed notice lines

```
mcp__systemprompt__render_artifact {"artifact_type": "message"}
```

Info, warning, and success notices rendered from structured data.

### 8. Text — how all of this works

```
mcp__systemprompt__render_artifact {"artifact_type": "text"}
```

### 9. Close — let the spine state the count

This step is not optional. Before writing any summary, call:

```
mcp__systemprompt__governance_stats {}
```

Read the `render_artifact` allow count back to the viewer, quoting the number
the spine returned — not the number of sections in this file, and not your own
tally. Eight `allow` verdicts means the shelf holds one artifact of every
type; anything less is the more interesting result, and is worth showing:
say which calls landed, which did not, and point at the audit trail as the
proof.
