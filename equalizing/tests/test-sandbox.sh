#!/usr/bin/env bash
# End-to-end test for the dev sandbox environment.
# Run inside the sandbox:  nix run .#sandbox -- --run bash tests/test-sandbox.sh
set -euo pipefail

PASS=0
FAIL=0

pass() { PASS=$((PASS + 1)); echo "  PASS: $1"; }
fail() { FAIL=$((FAIL + 1)); echo "  FAIL: $1"; }
check() {
  local desc="$1"; shift
  if "$@" >/dev/null 2>&1; then pass "$desc"; else fail "$desc"; fi
}

# --- bijective safe-path encoding (base32) ---
# Maps arbitrary strings to filesystem-safe names and back.
to_safe()   { printf '%s' "$1" | base32 | tr -d '\n'; }
from_safe() { printf '%s' "$1" | base32 -d; }

# --- resolve env vars (allow running outside sandbox too) ---
PROJECT_ROOT="${PROJECT_ROOT:-$(cd "$(dirname "$0")/.." && pwd)}"
DATA_DIR="${DATA_DIR:-$PROJECT_ROOT/.dev-data}"
PGDATA="${PGDATA:-$DATA_DIR/postgres}"
PGHOST="${PGHOST:-$PGDATA}"
PGPORT="${PGPORT:-5432}"
PGDATABASE="${PGDATABASE:-synapse}"
SYNAPSE_DATA="${SYNAPSE_DATA:-$DATA_DIR/synapse}"
SYNAPSE_CONFIG="${SYNAPSE_CONFIG:-$SYNAPSE_DATA/homeserver.yaml}"
SYNAPSE_SERVER_NAME="${SYNAPSE_SERVER_NAME:-localhost}"
SYNAPSE_PORT="${SYNAPSE_PORT:-8008}"

export PGDATA PGHOST PGPORT PGDATABASE

SYNAPSE_URL="http://127.0.0.1:$SYNAPSE_PORT"
CLEANUP_PG=0
CLEANUP_SYNAPSE=0

# Per-run temp directory for all test artifacts
TEST_TMP=$(mktemp -d "$DATA_DIR/test-run-XXXXXX")

cleanup() {
  echo ""
  echo "=== Cleanup ==="
  if [ "${SKIP_CLEANUP:-0}" = 1 ]; then
    echo "  Skipped (SKIP_CLEANUP=1)"
    rm -rf "$TEST_TMP"
    echo "  Test artifacts removed ($TEST_TMP)"
    return
  fi
  if [ "$CLEANUP_SYNAPSE" = 1 ]; then
    if [ "${MC_SANDBOXED:-0}" = 1 ]; then
      # Safe inside PID-namespaced sandbox
      pkill -f 'synapse.app.homeserver' 2>/dev/null || true
    elif [ -f "$SYNAPSE_DATA/homeserver.pid" ]; then
      # Outside sandbox: use Synapse's own PID file to avoid killing unrelated processes
      kill "$(cat "$SYNAPSE_DATA/homeserver.pid")" 2>/dev/null || true
    fi
    echo "  Synapse stopped"
  fi
  if [ "$CLEANUP_PG" = 1 ] && pg_isready -q 2>/dev/null; then
    pg_ctl -D "$PGDATA" stop -m fast 2>/dev/null || true
    echo "  PostgreSQL stopped"
  fi
  rm -rf "$TEST_TMP"
  echo "  Test artifacts removed ($TEST_TMP)"
}
trap cleanup EXIT

# ============================================================
echo "=== Test: Tool availability ==="
# ============================================================
check "bash available"           command -v bash
check "python available"         command -v python
check "cargo available"          command -v cargo
check "rustc available"          command -v rustc
check "synapse_homeserver"       command -v synapse_homeserver
check "register_new_matrix_user" command -v register_new_matrix_user
check "pg_ctl available"         command -v pg_ctl
check "initdb available"         command -v initdb
check "jq available"             command -v jq
check "curl available"           command -v curl

# ============================================================
echo ""
echo "=== Test: Python imports ==="
# ============================================================
check "import nio"       python -c "import nio"
check "import nio.crypto (olm/e2ee)" python -c "from nio.crypto import OlmDevice"
check "import aiohttp"   python -c "import aiohttp"
check "import aiofiles"  python -c "import aiofiles"
check "import emoji"     python -c "import emoji"
check "import markdown"  python -c "import markdown"
check "import PIL"       python -c "from PIL import Image"
check "import magic"     python -c "import magic"
check "import requests"  python -c "import requests"
check "import pytest"    python -c "import pytest"

# ============================================================
echo ""
echo "=== Test: PostgreSQL ==="
# ============================================================
mkdir -p "$DATA_DIR"

if [ ! -d "$PGDATA/base" ]; then
  echo "  Initializing PostgreSQL..."
  initdb --no-locale --encoding=UTF8 -D "$PGDATA" >/dev/null 2>&1
  printf "unix_socket_directories = '%s'\n" "$PGDATA" >> "$PGDATA/postgresql.conf"
  printf "listen_addresses = ''\n" >> "$PGDATA/postgresql.conf"
  printf "port = %s\n" "$PGPORT" >> "$PGDATA/postgresql.conf"
fi

if ! pg_isready -q 2>/dev/null; then
  pg_ctl -D "$PGDATA" -l "$DATA_DIR/postgres.log" start -w >/dev/null 2>&1
  CLEANUP_PG=1
fi

check "pg_isready" pg_isready -q
createuser --no-superuser --no-createdb --no-createrole synapse 2>/dev/null || true
createdb --owner=synapse synapse 2>/dev/null || true
check "synapse db exists" psql -d synapse -c "SELECT 1" -t -A

# ============================================================
echo ""
echo "=== Test: Synapse ==="
# ============================================================
mkdir -p "$SYNAPSE_DATA"

if [ ! -f "$SYNAPSE_CONFIG" ]; then
  echo "  Generating Synapse config..."
  synapse_homeserver \
    --server-name "$SYNAPSE_SERVER_NAME" \
    --config-path "$SYNAPSE_CONFIG" \
    --data-directory "$SYNAPSE_DATA" \
    --generate-config \
    --report-stats=no >/dev/null 2>&1
fi
check "homeserver.yaml exists" test -f "$SYNAPSE_CONFIG"

# Write override config
cat > "$SYNAPSE_DATA/homeserver-override.yaml" <<YAML
database:
  name: psycopg2
  args:
    user: synapse
    database: synapse
    host: $PGDATA
    cp_min: 1
    cp_max: 5

rc_message:
  per_second: 1000
  burst_count: 1000
rc_registration:
  per_second: 1000
  burst_count: 1000
rc_login:
  address:
    per_second: 1000
    burst_count: 1000
  account:
    per_second: 1000
    burst_count: 1000
  failed_attempts:
    per_second: 1000
    burst_count: 1000

enable_registration: true
enable_registration_without_verification: true

listeners:
  - port: $SYNAPSE_PORT
    type: http
    tls: false
    bind_addresses: ['127.0.0.1']
    resources:
      - names: [client, federation]
        compress: false

suppress_key_server_warning: true
YAML

echo "  Starting Synapse..."
synapse_homeserver \
  --config-path "$SYNAPSE_CONFIG" \
  --config-path "$SYNAPSE_DATA/homeserver-override.yaml" \
  -D 2>&1
CLEANUP_SYNAPSE=1

# Wait for Synapse to be ready (up to 15 seconds)
SYNAPSE_READY=0
for i in $(seq 1 30); do
  if curl -sf "$SYNAPSE_URL/_matrix/client/versions" >/dev/null 2>&1; then
    SYNAPSE_READY=1
    break
  fi
  sleep 0.5
done

if [ "$SYNAPSE_READY" = 0 ]; then
  fail "synapse responds (timed out after 15s)"
  echo "FATAL: Synapse did not start. Aborting."
  exit 1
fi
pass "synapse responds"

VERSIONS_RESP=$(curl -sf "$SYNAPSE_URL/_matrix/client/versions")
VERSIONS=$(echo "$VERSIONS_RESP" | jq -r '.versions[]' 2>/dev/null | head -1)
if [ -n "$VERSIONS" ]; then pass "synapse returns versions ($VERSIONS)"; else fail "synapse returns versions"; fi

# ============================================================
echo ""
echo "=== Test: User registration ==="
# ============================================================
register_new_matrix_user \
  -u alice -p alicepass \
  -c "$SYNAPSE_CONFIG" --no-admin \
  "$SYNAPSE_URL" 2>/dev/null

register_new_matrix_user \
  -u bob -p bobpass \
  -c "$SYNAPSE_CONFIG" --no-admin \
  "$SYNAPSE_URL" 2>/dev/null

register_new_matrix_user \
  -u admin -p adminpass \
  -c "$SYNAPSE_CONFIG" --admin \
  "$SYNAPSE_URL" 2>/dev/null

# ============================================================
echo ""
echo "=== Test: Matrix client API ==="
# ============================================================

# Login as alice, get access token (also verifies registration)
ALICE_RESP=$(curl -sf -X POST "$SYNAPSE_URL/_matrix/client/r0/login" \
  -H "Content-Type: application/json" \
  -d '{"type":"m.login.password","user":"alice","password":"alicepass"}')
ALICE_TOKEN=$(echo "$ALICE_RESP" | jq -r '.access_token')
ALICE_DEVICE=$(echo "$ALICE_RESP" | jq -r '.device_id')

if [ "$ALICE_TOKEN" != "null" ] && [ -n "$ALICE_TOKEN" ]; then
  pass "alice registered + login (device=$ALICE_DEVICE)"
else
  fail "alice registered + login"
fi

# Login as bob (also verifies registration)
BOB_RESP=$(curl -sf -X POST "$SYNAPSE_URL/_matrix/client/r0/login" \
  -H "Content-Type: application/json" \
  -d '{"type":"m.login.password","user":"bob","password":"bobpass"}')
BOB_TOKEN=$(echo "$BOB_RESP" | jq -r '.access_token')

if [ "$BOB_TOKEN" != "null" ] && [ -n "$BOB_TOKEN" ]; then
  pass "bob registered + login"
else
  fail "bob registered + login"
fi

# Alice creates a room
ROOM_RESP=$(curl -sf -X POST "$SYNAPSE_URL/_matrix/client/r0/createRoom" \
  -H "Authorization: Bearer $ALICE_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"name":"test-room","invite":["@bob:localhost"]}')
ROOM_ID=$(echo "$ROOM_RESP" | jq -r '.room_id')

if [ "$ROOM_ID" != "null" ] && [ -n "$ROOM_ID" ]; then
  pass "alice created room ($ROOM_ID)"
else
  fail "alice created room"
fi

# Bob joins the room
JOIN_RESP=$(curl -sf -X POST "$SYNAPSE_URL/_matrix/client/r0/join/$ROOM_ID" \
  -H "Authorization: Bearer $BOB_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{}')
if echo "$JOIN_RESP" | jq -e '.room_id' >/dev/null 2>&1; then
  pass "bob joined room"
else
  fail "bob joined room"
fi

# Alice sends a message
MSG_RESP=$(curl -sf -X POST "$SYNAPSE_URL/_matrix/client/r0/rooms/$ROOM_ID/send/m.room.message" \
  -H "Authorization: Bearer $ALICE_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"msgtype":"m.text","body":"hello from alice"}')
if echo "$MSG_RESP" | jq -e '.event_id' >/dev/null 2>&1; then
  pass "alice sent message"
else
  fail "alice sent message"
fi

# (room message verification deferred until after all messages are sent)

# ============================================================
echo ""
echo "=== Test: Media upload/download (image) ==="
# ============================================================

# Generate a test PNG (100x100 red square)
python -c "
from PIL import Image
img = Image.new('RGB', (100, 100), color=(255, 0, 0))
img.save('$TEST_TMP/image.png')
"
check "test PNG created" test -f "$TEST_TMP/image.png"
IMG_SIZE_ORIG=$(wc -c < "$TEST_TMP/image.png")

# Alice uploads the image
IMG_UPLOAD_RESP=$(curl -sf -X POST "$SYNAPSE_URL/_matrix/media/v3/upload?filename=test-image.png" \
  -H "Authorization: Bearer $ALICE_TOKEN" \
  -H "Content-Type: image/png" \
  --data-binary "@$TEST_TMP/image.png")
IMG_MXC=$(echo "$IMG_UPLOAD_RESP" | jq -r '.content_uri')

if [ "$IMG_MXC" != "null" ] && [ -n "$IMG_MXC" ]; then
  pass "alice uploaded image ($IMG_MXC)"
else
  fail "alice uploaded image"
fi

# Extract media ID and encode to safe filename for download
IMG_MEDIA_PATH=$(echo "$IMG_MXC" | sed 's|mxc://||')   # server/id
IMG_MEDIA_ID=$(echo "$IMG_MXC" | sed 's|mxc://[^/]*/||') # just id
IMG_DL_NAME=$(to_safe "$IMG_MEDIA_ID").png

# Alice sends the image as a message in the room
IMG_MSG_RESP=$(curl -sf -X POST "$SYNAPSE_URL/_matrix/client/r0/rooms/$ROOM_ID/send/m.room.message" \
  -H "Authorization: Bearer $ALICE_TOKEN" \
  -H "Content-Type: application/json" \
  -d "{\"msgtype\":\"m.image\",\"body\":\"test-image.png\",\"url\":\"$IMG_MXC\",\"info\":{\"mimetype\":\"image/png\",\"w\":100,\"h\":100,\"size\":$IMG_SIZE_ORIG}}")
if echo "$IMG_MSG_RESP" | jq -e '.event_id' >/dev/null 2>&1; then
  pass "alice sent image message"
else
  fail "alice sent image message"
fi

# Bob downloads the image (authenticated endpoint, safe filename)
curl -sf "$SYNAPSE_URL/_matrix/client/v1/media/download/$IMG_MEDIA_PATH" \
  -H "Authorization: Bearer $BOB_TOKEN" \
  -o "$TEST_TMP/$IMG_DL_NAME"
IMG_SIZE_DL=$(wc -c < "$TEST_TMP/$IMG_DL_NAME" 2>/dev/null || echo 0)

if [ "$IMG_SIZE_DL" -eq "$IMG_SIZE_ORIG" ]; then
  pass "bob downloaded image (${IMG_SIZE_DL} bytes, matches original)"
else
  fail "bob downloaded image (got ${IMG_SIZE_DL} bytes, expected ${IMG_SIZE_ORIG})"
fi

# Verify image is valid by loading it with PIL
check "downloaded image is valid PNG" python -c "
from PIL import Image
img = Image.open('$TEST_TMP/$IMG_DL_NAME')
assert img.size == (100, 100), f'wrong size: {img.size}'
assert img.getpixel((50, 50)) == (255, 0, 0), f'wrong color: {img.getpixel((50, 50))}'
"

# Verify round-trip: decode the safe filename back to the original ID
IMG_ID_ROUNDTRIP=$(from_safe "${IMG_DL_NAME%.png}")
if [ "$IMG_ID_ROUNDTRIP" = "$IMG_MEDIA_ID" ]; then
  pass "image filename round-trips through base32 ($IMG_MEDIA_ID)"
else
  fail "image filename round-trip (got '$IMG_ID_ROUNDTRIP', expected '$IMG_MEDIA_ID')"
fi

# Check media is stored on disk (synapse layout: <id[0:2]>/<id[2:4]>/<id[4:]>)
IMG_STORED="$SYNAPSE_DATA/media_store/local_content/${IMG_MEDIA_ID:0:2}/${IMG_MEDIA_ID:2:2}/${IMG_MEDIA_ID:4}"
if [ -f "$IMG_STORED" ]; then
  pass "image stored on disk ($IMG_STORED)"
else
  fail "image stored on disk (expected $IMG_STORED)"
fi

# Check thumbnails were generated
if [ -n "$(find "$SYNAPSE_DATA/media_store/local_thumbnails" -type f -print -quit 2>/dev/null)" ]; then
  pass "image thumbnails generated"
else
  fail "image thumbnails generated"
fi

# (image room history check deferred until after all messages are sent)

# ============================================================
echo ""
echo "=== Test: Media upload/download (audio) ==="
# ============================================================

# Generate a test WAV (0.5s of 440Hz sine wave)
python -c "
import struct, wave, math
f = wave.open('$TEST_TMP/audio.wav', 'w')
f.setnchannels(1)
f.setsampwidth(2)
f.setframerate(44100)
for i in range(22050):
    v = int(32767 * math.sin(2 * math.pi * 440 * i / 44100))
    f.writeframes(struct.pack('<h', v))
f.close()
"
check "test WAV created" test -f "$TEST_TMP/audio.wav"
AUDIO_SIZE_ORIG=$(wc -c < "$TEST_TMP/audio.wav")

# Alice uploads the audio
AUDIO_UPLOAD_RESP=$(curl -sf -X POST "$SYNAPSE_URL/_matrix/media/v3/upload?filename=test-audio.wav" \
  -H "Authorization: Bearer $ALICE_TOKEN" \
  -H "Content-Type: audio/wav" \
  --data-binary "@$TEST_TMP/audio.wav")
AUDIO_MXC=$(echo "$AUDIO_UPLOAD_RESP" | jq -r '.content_uri')

if [ "$AUDIO_MXC" != "null" ] && [ -n "$AUDIO_MXC" ]; then
  pass "alice uploaded audio ($AUDIO_MXC)"
else
  fail "alice uploaded audio"
fi

# Extract media ID and encode to safe filename
AUDIO_MEDIA_PATH=$(echo "$AUDIO_MXC" | sed 's|mxc://||')
AUDIO_MEDIA_ID=$(echo "$AUDIO_MXC" | sed 's|mxc://[^/]*/||')
AUDIO_DL_NAME=$(to_safe "$AUDIO_MEDIA_ID").wav

# Alice sends audio as a message
AUDIO_MSG_RESP=$(curl -sf -X POST "$SYNAPSE_URL/_matrix/client/r0/rooms/$ROOM_ID/send/m.room.message" \
  -H "Authorization: Bearer $ALICE_TOKEN" \
  -H "Content-Type: application/json" \
  -d "{\"msgtype\":\"m.audio\",\"body\":\"test-audio.wav\",\"url\":\"$AUDIO_MXC\",\"info\":{\"mimetype\":\"audio/wav\",\"duration\":500,\"size\":$AUDIO_SIZE_ORIG}}")
if echo "$AUDIO_MSG_RESP" | jq -e '.event_id' >/dev/null 2>&1; then
  pass "alice sent audio message"
else
  fail "alice sent audio message"
fi

# Bob downloads the audio (safe filename)
curl -sf "$SYNAPSE_URL/_matrix/client/v1/media/download/$AUDIO_MEDIA_PATH" \
  -H "Authorization: Bearer $BOB_TOKEN" \
  -o "$TEST_TMP/$AUDIO_DL_NAME"
AUDIO_SIZE_DL=$(wc -c < "$TEST_TMP/$AUDIO_DL_NAME" 2>/dev/null || echo 0)

if [ "$AUDIO_SIZE_DL" -eq "$AUDIO_SIZE_ORIG" ]; then
  pass "bob downloaded audio (${AUDIO_SIZE_DL} bytes, matches original)"
else
  fail "bob downloaded audio (got ${AUDIO_SIZE_DL} bytes, expected ${AUDIO_SIZE_ORIG})"
fi

# Verify audio is valid WAV
check "downloaded audio is valid WAV" python -c "
import wave
f = wave.open('$TEST_TMP/$AUDIO_DL_NAME', 'r')
assert f.getnchannels() == 1, f'wrong channels: {f.getnchannels()}'
assert f.getframerate() == 44100, f'wrong rate: {f.getframerate()}'
assert f.getnframes() == 22050, f'wrong frames: {f.getnframes()}'
f.close()
"

# Verify round-trip
AUDIO_ID_ROUNDTRIP=$(from_safe "${AUDIO_DL_NAME%.wav}")
if [ "$AUDIO_ID_ROUNDTRIP" = "$AUDIO_MEDIA_ID" ]; then
  pass "audio filename round-trips through base32 ($AUDIO_MEDIA_ID)"
else
  fail "audio filename round-trip (got '$AUDIO_ID_ROUNDTRIP', expected '$AUDIO_MEDIA_ID')"
fi

# Check audio is stored on disk
AUDIO_STORED="$SYNAPSE_DATA/media_store/local_content/${AUDIO_MEDIA_ID:0:2}/${AUDIO_MEDIA_ID:2:2}/${AUDIO_MEDIA_ID:4}"
if [ -f "$AUDIO_STORED" ]; then
  pass "audio stored on disk ($AUDIO_STORED)"
else
  fail "audio stored on disk (expected $AUDIO_STORED)"
fi

# ============================================================
echo ""
echo "=== Test: Room message history (single fetch) ==="
# ============================================================

# Fetch room messages once and verify all three message types
ROOM_MSGS=$(curl -sf "$SYNAPSE_URL/_matrix/client/r0/rooms/$ROOM_ID/messages?dir=b&limit=10" \
  -H "Authorization: Bearer $BOB_TOKEN")
if echo "$ROOM_MSGS" | jq -e '.chunk[] | select(.content.body == "hello from alice")' >/dev/null 2>&1; then
  pass "bob received alice's text message"
else
  fail "bob received alice's text message"
fi
if echo "$ROOM_MSGS" | jq -e '.chunk[] | select(.content.msgtype == "m.image" and .content.body == "test-image.png")' >/dev/null 2>&1; then
  pass "bob sees image message in room history"
else
  fail "bob sees image message in room history"
fi
if echo "$ROOM_MSGS" | jq -e '.chunk[] | select(.content.msgtype == "m.audio" and .content.body == "test-audio.wav")' >/dev/null 2>&1; then
  pass "bob sees audio message in room history"
else
  fail "bob sees audio message in room history"
fi

# ============================================================
echo ""
echo "=== Test: Rust build environment ==="
# ============================================================
check "cargo version"  cargo --version
check "rustc version"  rustc --version
check "pkg-config finds openssl" pkg-config --exists openssl
check "pkg-config finds sqlite3" pkg-config --exists sqlite3

# ============================================================
echo ""
echo "========================================"
echo "  Results: $PASS passed, $FAIL failed"
echo "========================================"
[ "$FAIL" -eq 0 ]
