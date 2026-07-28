# Hand-run production migration scripts

Nothing executes this directory. The migrations the server actually runs live
in `extensions/web/schema/migrations/NNN_*.sql`, are compiled into the binary
via each crate's `build.rs`, and are applied by `systemprompt infra db migrate`
(the Docker entrypoint runs it on boot). Numbering gaps there are historical
and load-bearing — never renumber; see the migrations section of the schema
linter docs.

Files here are one-off, forward-only, idempotent scripts an operator applies
by hand to long-lived databases (systemprompt-prod / FlyIO) that predate a
schema change the compiled migrations assume. Date-name them
(`YYYY-MM-DD-<what>.sql`), keep them re-runnable (`IF NOT EXISTS` /
`IF EXISTS`), and delete nothing — the directory is the audit trail of what
prod was hand-fed.
