# AI Governance Policy (Excerpt)

## Scope

This policy applies to all automated agents and human operators invoking AI
tools through the systemprompt.io platform. It is enforced at runtime by the
governance hook pipeline (`PreToolUse` and `PostToolUse`).

## Principles

1. **Least privilege**: agents are granted the narrowest scope sufficient for
   their role (`user`, `service`, `admin`).
2. **Secrets never traverse prompts**: plaintext credentials, API keys, and
   tokens must never appear in tool input. The governance hook denies any
   invocation whose input matches a known secret pattern.
3. **Destructive operations require admin scope**: any tool whose name
   contains `delete`, `drop`, or `destroy` is restricted to the `admin` scope.
4. **Every decision is auditable**: allow, deny, warn, and rate-limit events
   are persisted to `governance_decisions` with session, agent, tool, rule,
   and reason fields.

## Incident response

Contact the platform team if a decision looks wrong or if you see repeated
`rate_limit` denials for a trusted workflow.
