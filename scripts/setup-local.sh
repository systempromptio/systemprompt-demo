#!/usr/bin/env bash
set -euo pipefail
ROOT="$(git rev-parse --show-toplevel)"
PROFILE_DIR="$ROOT/.systemprompt/profiles/local"
DOCKER_DIR="$ROOT/.systemprompt/docker"
ANTHROPIC_KEY="${1:-}"
OPENAI_KEY="${2:-}"
GEMINI_KEY="${3:-}"
HTTP_PORT="${4:-8080}"
PG_PORT="${5:-5432}"
export SYSTEMPROMPT_PROFILE="$PROFILE_DIR/profile.yaml"
# Whether a key was passed as a positional arg. When none is and there is
# nothing to preserve, generation still needs a provider: on a TTY we let
# `admin setup` drive its own "Select your AI provider" menu (the CLI owns
# the prompt); off a TTY we cannot prompt, so keys must come as args. A
# developer who keeps .systemprompt/ across reclones re-runs with no args
# and is never asked again (the profile.yaml guard below skips generation).
HAS_KEY=false
if [ -n "$ANTHROPIC_KEY" ] || [ -n "$OPENAI_KEY" ] || [ -n "$GEMINI_KEY" ]; then
    HAS_KEY=true
fi
if [ "$HAS_KEY" = false ] && [ ! -f "$PROFILE_DIR/secrets.json" ] && [ ! -t 0 ]; then
    echo ""
    echo "================================================================"
    echo "  setup-local needs an AI provider API key"
    echo "================================================================"
    echo ""
    echo "  Not running on a TTY, so the provider menu can't be shown."
    echo "  Pass a key as an argument (one of Anthropic, OpenAI, Gemini):"
    echo "    just setup-local <anthropic_key> [openai_key] [gemini_key]"
    echo ""
    exit 1
fi
if [ ! -x target/debug/systemprompt ] && [ ! -x target/release/systemprompt ]; then
    echo "Building debug binary..."
    just build
fi
# Resolve the binary at runtime: the {{CLI}} variable is evaluated by `just`
# at parse time, so on a cold clone (no binary yet) it expands to an error
# stub — useless for the bootstrap/keygen calls below, which run only after
# the build above has produced the binary.
if [ -x target/release/systemprompt ]; then
    BIN="$ROOT/target/release/systemprompt"
else
    BIN="$ROOT/target/debug/systemprompt"
fi
mkdir -p "$PROFILE_DIR" "$DOCKER_DIR"
if [ ! -f "$DOCKER_DIR/local.yaml" ]; then
    echo "Writing Docker compose for local Postgres (host port $PG_PORT)..."
    cat > "$DOCKER_DIR/local.yaml" <<YAML
services:
  postgres:
    image: postgres:18-alpine
    restart: unless-stopped
    environment:
      POSTGRES_USER: systemprompt
      POSTGRES_PASSWORD: 123
      POSTGRES_DB: systemprompt
    ports:
      - "${PG_PORT}:5432"
    volumes:
      - postgres_data:/var/lib/postgresql
    healthcheck:
      test: ["CMD-SHELL", "pg_isready -U systemprompt -d systemprompt"]
      interval: 5s
      timeout: 5s
      retries: 5
volumes:
  postgres_data: {}
YAML
fi
if [ ! -f "$PROFILE_DIR/profile.yaml" ]; then
    echo "Generating profile + provider registry + secrets via 'admin setup'..."
    if [ "$HAS_KEY" = true ]; then
        # Keys supplied as args: fully non-interactive. The default provider
        # is the first key given, so the generated config (the providers
        # registry, gateway default, ai/config.yaml) is consistent with the
        # single key.
        KEY_ARGS=()
        DEFAULT_PROVIDER=""
        if [ -n "$ANTHROPIC_KEY" ]; then KEY_ARGS+=(--anthropic-key "$ANTHROPIC_KEY"); [ -z "$DEFAULT_PROVIDER" ] && DEFAULT_PROVIDER=anthropic; fi
        if [ -n "$OPENAI_KEY" ]; then KEY_ARGS+=(--openai-key "$OPENAI_KEY"); [ -z "$DEFAULT_PROVIDER" ] && DEFAULT_PROVIDER=openai; fi
        if [ -n "$GEMINI_KEY" ]; then KEY_ARGS+=(--gemini-key "$GEMINI_KEY"); [ -z "$DEFAULT_PROVIDER" ] && DEFAULT_PROVIDER=gemini; fi
        "$BIN" admin setup --yes --no-migrate --environment local \
            --db-host localhost --db-port "$PG_PORT" \
            --db-user systemprompt --db-password 123 --db-name systemprompt \
            --default-provider "$DEFAULT_PROVIDER" \
            "${KEY_ARGS[@]}"
    else
        # No key arg: let the CLI prompt for which provider to use. DB,
        # environment, and migrations stay non-interactive (flags + env);
        # only the provider selection is interactive, and the chosen
        # provider becomes the default.
        SYSTEMPROMPT_NON_INTERACTIVE=1 "$BIN" admin setup --no-migrate --environment local \
            --db-host localhost --db-port "$PG_PORT" \
            --db-user systemprompt --db-password 123 --db-name systemprompt
    fi
    if [ "$HTTP_PORT" != "8080" ]; then
        "$BIN" admin config server set --port "$HTTP_PORT" \
            --api-server-url "http://localhost:${HTTP_PORT}" \
            --api-internal-url "http://localhost:${HTTP_PORT}" \
            --api-external-url "http://localhost:${HTTP_PORT}"
        # The authz hook URL is an absolute webhook target baked at
        # `admin setup` time on the default port; re-point it at the
        # chosen port so the gateway's govern callback reaches this server.
        "$BIN" admin config governance set --mode webhook \
            --url "http://localhost:${HTTP_PORT}/api/public/govern/authz"
    fi
elif [ "$HAS_KEY" = true ]; then
    # Profile generation is one-shot, guarded on profile.yaml. `just db-down`
    # drops the database but leaves the profile, so a re-run with different
    # keys would silently keep the old provider registry. Say so loudly and
    # point at the one command that actually re-provisions.
    echo ""
    echo "================================================================"
    echo "  Existing profile reused — supplied keys were NOT applied"
    echo "================================================================"
    echo ""
    echo "  $PROFILE_DIR/profile.yaml already exists, so 'admin setup' was"
    echo "  skipped and the provider registry/keys are unchanged."
    echo "  To re-provision from the keys you just passed:"
    echo ""
    echo "    rm -rf \"$PROFILE_DIR\" && just setup-local <keys...> $HTTP_PORT $PG_PORT"
    echo ""
fi
mkdir -p "$ROOT/web/dist"
echo "Building binaries (release, full workspace)..."
just build --release
echo "Starting local Postgres via Docker..."
just db-up local
echo "Waiting for Postgres to accept connections on localhost:${PG_PORT}..."
for i in $(seq 1 60); do
    if (exec 3<>/dev/tcp/127.0.0.1/${PG_PORT}) 2>/dev/null; then
        exec 3<&- 3>&-
        # Also confirm the server actually answers pg_isready, not just a half-open socket.
        CONTAINER=$(docker compose -p "$(just _project_name local)" -f .systemprompt/docker/local.yaml ps -q postgres)
        if [ -n "$CONTAINER" ] && docker exec "$CONTAINER" pg_isready -U systemprompt -d systemprompt >/dev/null 2>&1; then
            echo "Postgres is ready."
            break
        fi
    fi
    if [ "$i" = "60" ]; then
        echo "ERROR: Postgres did not become ready within 60s." >&2
        exit 1
    fi
    sleep 1
done
echo "Running database migrations..."
just migrate
echo "Ensuring bootstrap admin user..."
"$BIN" admin bootstrap
if [ ! -f "$ROOT/signing_key.pem" ]; then
    echo "Generating JWT signing key..."
    "$BIN" admin keys generate --output "$ROOT/signing_key.pem"
fi
echo "Publishing assets..."
just publish
echo ""
echo "Local setup complete. Run: just start"

# List all tenants
