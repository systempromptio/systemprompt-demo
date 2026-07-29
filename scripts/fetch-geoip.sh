#!/usr/bin/env bash
# Download the MaxMind GeoLite2-City database that backs IP -> country
# analytics (`analytics traffic geo`, the country column on user_sessions).
#
# Opt-in, never run by setup or by the image build. GeoIP is off unless an
# operator both fetches a database and points paths.geoip_database at it;
# without one the server logs a startup notice and records sessions with a
# NULL country, which is the default for a fresh clone of this template.
#
# GeoLite2 requires your own MaxMind account: sign up (free) at
# https://www.maxmind.com/en/geolite2/signup and export MAXMIND_LICENSE_KEY.
# --mirror instead pulls the CC BY-SA 4.0 redistribution at
# https://github.com/P3TERX/GeoLite.mmdb — read that licence before relying
# on it, since it is not covered by MaxMind's own terms of service.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DEST_DIR="${GEOIP_DIR:-$ROOT/storage/geoip}"
DEST="$DEST_DIR/GeoLite2-City.mmdb"
MIRROR_URL="https://github.com/P3TERX/GeoLite.mmdb/releases/latest/download/GeoLite2-City.mmdb"

FORCE=false
MIRROR=false
for arg in "$@"; do
    case "$arg" in
        --force) FORCE=true ;;
        --mirror) MIRROR=true ;;
        *) echo "Usage: fetch-geoip.sh [--force] [--mirror]" >&2; exit 2 ;;
    esac
done

if [ "$FORCE" = false ] && [ -s "$DEST" ]; then
    echo "GeoIP database already present: $DEST"
    exit 0
fi

if [ -z "${MAXMIND_LICENSE_KEY:-}" ] && [ "$MIRROR" = false ]; then
    cat >&2 <<'MSG'
MAXMIND_LICENSE_KEY is not set, so there is nothing licensed to download.

  Country analytics are optional. To enable them:
    1. Create a free MaxMind account:
       https://www.maxmind.com/en/geolite2/signup
    2. export MAXMIND_LICENSE_KEY=<your key> && just geoip
    3. Point paths.geoip_database at storage/geoip/GeoLite2-City.mmdb in
       your profile, then restart the server.

  Or pass --mirror to use the CC BY-SA 4.0 redistribution on GitHub instead
  of MaxMind's own distribution.
MSG
    exit 1
fi

mkdir -p "$DEST_DIR"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

if [ -n "${MAXMIND_LICENSE_KEY:-}" ]; then
    echo "Downloading GeoLite2-City from MaxMind..."
    curl -fsSL -o "$TMP/geoip.tar.gz" \
        "https://download.maxmind.com/app/geoip_download?edition_id=GeoLite2-City&license_key=${MAXMIND_LICENSE_KEY}&suffix=tar.gz"
    tar -xzf "$TMP/geoip.tar.gz" -C "$TMP"
    # The archive unpacks into a dated directory: GeoLite2-City_20260729/.
    find "$TMP" -name 'GeoLite2-City.mmdb' -exec cp {} "$TMP/out.mmdb" \;
else
    echo "Downloading GeoLite2-City from the public CC BY-SA mirror..."
    curl -fsSL -o "$TMP/out.mmdb" "$MIRROR_URL"
fi

if [ ! -s "$TMP/out.mmdb" ]; then
    echo "ERROR: GeoLite2-City.mmdb was not produced by the download." >&2
    exit 1
fi

mv "$TMP/out.mmdb" "$DEST"
echo "GeoIP database written: $DEST ($(wc -c < "$DEST") bytes)"
echo "Set paths.geoip_database to this path in your profile to enable it."
