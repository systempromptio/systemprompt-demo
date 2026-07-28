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

## How to Use

Call `render_artifact` once per type, in this order. Say one sentence about
each before calling it; after the call, tell the viewer to open the result
from the shelf. The first three are built from **this session's own governance
spine** — the same rows `governance_stats` reports — so they change as the
session does. The rest carry curated content.

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

Close by noting the shelf now holds one artifact of every type, and that every
one of these calls is on the audit spine — `governance_stats` will show eight
`allow` verdicts for `render_artifact`.
