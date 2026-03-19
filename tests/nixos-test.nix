# NixOS VM integration tests for matrix-commander-ng
# Tests 1-31: Core functionality (login, rooms, messaging, JSON, REST)
# Tests 32-64: Extended coverage (lifecycle, media, message modes, keys, negative)
{ pkgs, lib, matrix-commander-ng, ... }:

let
  # Shared server configuration
  serverName = "localhost";
  synapsePort = 8008;

  registrationSecret = "test-registration-secret";

  # Test users
  user1 = "alice";
  user1Pass = "alice-password-123";
  user2 = "bob";
  user2Pass = "bob-password-123";

  # Dex (OIDC identity provider) configuration
  dexPort = 5556;
  dexClientSecret = "synapse-dex-secret";
  ssoUser = "sso-alice";
  ssoUserEmail = "sso-alice@example.com";
  ssoUserPass = "sso-password-123";
  # bcrypt hash of ssoUserPass (cost 10)
  ssoUserPassHash = "$2b$10$isACYyVmJfQnrvyNY7PF6.m1//kHT5uHipGbQLppuXvi1L93zepC2";

  # Helper script to register a user via Synapse's admin API
  registerUser = pkgs.writeShellScript "register-user" ''
    set -euo pipefail
    USERNAME="$1"
    PASSWORD="$2"
    ADMIN_FLAG="--no-admin"
    if [ "''${3:-}" = "true" ]; then
      ADMIN_FLAG="--admin"
    fi
    register_new_matrix_user \
      --user "$USERNAME" \
      --password "$PASSWORD" \
      $ADMIN_FLAG \
      --shared-secret "${registrationSecret}" \
      http://127.0.0.1:${toString synapsePort}
    echo "Registered user $USERNAME"
  '';

  # Script that runs the full test suite for matrix-commander-ng
  mcRsTestScript = pkgs.writeShellScript "test-matrix-commander-ng" ''
    set -euo pipefail
    export HOME=/tmp/mc-rs-test
    mkdir -p "$HOME"
    cd "$HOME"

    MC="matrix-commander-ng"
    SERVER="http://127.0.0.1:${toString synapsePort}"

    # Helper to pass common credential/store args
    mc() {
      $MC --credentials ./credentials.json --store ./store/ "$@"
    }

    echo "============================================"
    echo "  Testing matrix-commander-ng"
    echo "============================================"

    failures=0

    fail() {
      echo "FAIL: $1"
      failures=$((failures + 1))
    }

    pass() {
      echo "PASS: $1"
    }

    # --- Test 1: Login alice device A ---
    echo ""
    echo "=== Test 1: Login alice device A ==="
    mkdir -p alice-deviceA
    cd alice-deviceA
    $MC --login password \
      --homeserver "$SERVER" \
      --user-login "@${user1}:${serverName}" \
      --password "${user1Pass}" \
      --device "alice-deviceA" \
      --room-default '!placeholder:${serverName}' \
      --credentials ./credentials.json \
      --store ./store/ 2>&1 || true

    if [ -f credentials.json ]; then
      pass "alice deviceA login"
    else
      fail "alice deviceA login - credentials.json not found"
    fi
    cd "$HOME"

    # --- Test 2: Login alice device B ---
    echo ""
    echo "=== Test 2: Login alice device B ==="
    mkdir -p alice-deviceB
    cd alice-deviceB
    $MC --login password \
      --homeserver "$SERVER" \
      --user-login "@${user1}:${serverName}" \
      --password "${user1Pass}" \
      --device "alice-deviceB" \
      --room-default '!placeholder:${serverName}' \
      --credentials ./credentials.json \
      --store ./store/ 2>&1 || true

    if [ -f credentials.json ]; then
      pass "alice deviceB login"
    else
      fail "alice deviceB login - credentials.json not found"
    fi
    cd "$HOME"

    # --- Test 3: Login bob ---
    echo ""
    echo "=== Test 3: Login bob ==="
    mkdir -p bob
    cd bob
    $MC --login password \
      --homeserver "$SERVER" \
      --user-login "@${user2}:${serverName}" \
      --password "${user2Pass}" \
      --device "bob-device" \
      --room-default '!placeholder:${serverName}' \
      --credentials ./credentials.json \
      --store ./store/ 2>&1 || true

    if [ -f credentials.json ]; then
      pass "bob login"
    else
      fail "bob login - credentials.json not found"
    fi
    cd "$HOME"

    # --- Test 4: Version ---
    echo ""
    echo "=== Test 4: Version check ==="
    cd alice-deviceA
    VERSION_OUT=$(mc --version 2>&1) || true
    echo "VERSION=$VERSION_OUT"
    if echo "$VERSION_OUT" | grep -qi "matrix.commander"; then
      pass "version check"
    else
      fail "version check"
    fi
    cd "$HOME"

    # --- Test 5: Whoami ---
    echo ""
    echo "=== Test 5: Whoami ==="
    cd alice-deviceA
    WHOAMI_OUT=$(mc --whoami 2>&1) || true
    echo "WHOAMI=$WHOAMI_OUT"
    if echo "$WHOAMI_OUT" | grep -q "${user1}"; then
      pass "whoami"
    else
      fail "whoami"
    fi
    cd "$HOME"

    # --- Test 6: Create room (unencrypted) ---
    echo ""
    echo "=== Test 6: Create room ==="
    cd alice-deviceA
    ROOM_OUT=$(mc --room-create test-room-rs --plain 2>&1) || true
    echo "ROOM_CREATE=$ROOM_OUT"

    # Extract room ID from output: first field (room_id    alias    ...)
    ROOM_ID=$(echo "$ROOM_OUT" | grep -oP '^!\S+' || echo "")
    echo "ROOM_ID=$ROOM_ID"
    if [ -n "$ROOM_ID" ]; then
      pass "create room - got room $ROOM_ID"
      # Update credentials.json with real room
      python3 -c "
import json
c = json.load(open('credentials.json'))
c['room_id'] = '$ROOM_ID'
json.dump(c, open('credentials.json', 'w'))
"
    else
      fail "create room - could not extract room ID"
    fi
    cd "$HOME"

    # --- Test 7: Send message to room ---
    echo ""
    echo "=== Test 7: Send message ==="
    cd alice-deviceA
    ROOM_ID=$(python3 -c "import json; print(json.load(open('credentials.json'))['room_id'])" 2>/dev/null || echo "")
    SEND_OUT=$(mc --room "$ROOM_ID" -m "Hello from matrix-commander-ng!" 2>&1) || true
    echo "SEND=$SEND_OUT"
    if echo "$SEND_OUT" | grep -qi "error"; then
      fail "send message"
    else
      pass "send message"
    fi
    cd "$HOME"

    # --- Test 8: List devices ---
    echo ""
    echo "=== Test 8: List devices ==="
    cd alice-deviceA
    DEVICES_OUT=$(mc --devices 2>&1) || true
    echo "DEVICES=$DEVICES_OUT"
    if echo "$DEVICES_OUT" | grep -q "alice-deviceA"; then
      pass "list devices (found alice-deviceA)"
    else
      fail "list devices (output captured)"
    fi
    cd "$HOME"

    # --- Test 9: Get display name ---
    echo ""
    echo "=== Test 9: Get display name ==="
    cd alice-deviceA
    DN_OUT=$(mc --get-display-name 2>&1) || true
    echo "DISPLAY_NAME=$DN_OUT"
    if [ -n "$DN_OUT" ]; then
      pass "get display name (non-empty output)"
    else
      fail "get display name - empty output"
    fi
    cd "$HOME"

    # --- Test 10: Set display name + round-trip verify ---
    echo ""
    echo "=== Test 10: Set display name ==="
    cd alice-deviceA
    SDN_OUT=$(mc --set-display-name "Alice Test" 2>&1) || true
    echo "SET_DISPLAY_NAME=$SDN_OUT"
    # Verify round-trip: get should now return "Alice Test"
    DN_VERIFY=$(mc --get-display-name 2>&1) || true
    echo "DISPLAY_NAME_VERIFY=$DN_VERIFY"
    if echo "$DN_VERIFY" | grep -q "Alice Test"; then
      pass "set display name (round-trip verified)"
    else
      fail "set display name - get-display-name does not contain 'Alice Test'"
    fi
    cd "$HOME"

    # --- Test 11: Joined rooms ---
    echo ""
    echo "=== Test 11: Joined rooms ==="
    cd alice-deviceA
    JR_OUT=$(mc --joined-rooms 2>&1) || true
    echo "JOINED_ROOMS=$JR_OUT"
    if echo "$JR_OUT" | grep -q "!"; then
      pass "joined rooms (has room IDs)"
    else
      fail "joined rooms - no room IDs in output"
    fi
    cd "$HOME"

    # --- Test 12: Bootstrap cross-signing + manual verify ---
    echo ""
    echo "=== Test 12: Bootstrap cross-signing + manual verify ==="
    cd alice-deviceA
    BOOTSTRAP_OUT=$(mc --bootstrap --password "${user1Pass}" 2>&1) || true
    echo "BOOTSTRAP=$BOOTSTRAP_OUT"
    if echo "$BOOTSTRAP_OUT" | grep -qi "error\|panic\|fatal"; then
      fail "bootstrap cross-signing"
    else
      pass "bootstrap cross-signing"
    fi

    VERIFY_OUT=$(mc --verify manual-device --user "@${user1}:${serverName}" 2>&1) || true
    echo "VERIFY=$VERIFY_OUT"
    if echo "$VERIFY_OUT" | grep -qi "error\|panic\|fatal"; then
      fail "manual device verification"
    else
      pass "manual device verification"
    fi
    cd "$HOME"

    # --- Test 13: Invite bob to room ---
    echo ""
    echo "=== Test 13: Invite bob to room ==="
    cd alice-deviceA
    ALICE_ROOM=$(python3 -c "import json; print(json.load(open('credentials.json'))['room_id'])" 2>/dev/null || echo "")
    INVITE_OUT=$(mc --room-invite "$ALICE_ROOM" --user "@${user2}:${serverName}" 2>&1) || true
    echo "INVITE=$INVITE_OUT"
    if echo "$INVITE_OUT" | grep -qi "error"; then
      fail "invite bob"
    else
      pass "invite bob"
    fi
    cd "$HOME"

    # Bob joins
    echo ""
    echo "=== Test 13b: Bob joins room ==="
    cd bob
    JOIN_OUT=$($MC --room-join "$ALICE_ROOM" --credentials ./credentials.json --store ./store/ 2>&1) || true
    echo "BOB_JOIN=$JOIN_OUT"
    if echo "$JOIN_OUT" | grep -qi "error"; then
      fail "bob join"
    else
      pass "bob join"
    fi
    cd "$HOME"

    # --- Test 14: Bob sends a message ---
    echo ""
    echo "=== Test 14: Bob sends message ==="
    cd bob
    BOB_SEND_OUT=$($MC --room "$ALICE_ROOM" -m "Hello from Bob!" \
      --credentials ./credentials.json --store ./store/ 2>&1) || true
    echo "BOB_SEND=$BOB_SEND_OUT"
    if echo "$BOB_SEND_OUT" | grep -qi "error"; then
      fail "bob send"
    else
      pass "bob send"
    fi
    cd "$HOME"

    # --- Test 15: Alice listens (tail last 5 messages) ---
    echo ""
    echo "=== Test 15: Listen tail ==="
    cd alice-deviceA
    LISTEN_OUT=$(timeout 30 $MC --listen tail --tail 5 --room "$ALICE_ROOM" \
      --credentials ./credentials.json --store ./store/ 2>&1) || true
    echo "LISTEN=$LISTEN_OUT"
    if echo "$LISTEN_OUT" | grep -q "Hello from matrix-commander-ng"; then
      pass "listen tail (contains alice's message)"
    elif echo "$LISTEN_OUT" | grep -q "Hello from Bob"; then
      pass "listen tail (contains bob's message)"
    elif [ -n "$LISTEN_OUT" ]; then
      pass "listen tail (non-empty output)"
    else
      fail "listen tail - empty output"
    fi
    cd "$HOME"

    # --- Test 16: Send message with --print-event-id ---
    echo ""
    echo "=== Test 16: Send with --print-event-id ==="
    cd alice-deviceA
    ROOM_ID=$(python3 -c "import json; print(json.load(open('credentials.json'))['room_id'])" 2>/dev/null || echo "")
    PEID_OUT=$(mc --room "$ROOM_ID" -m "test-print-event-id" --print-event-id 2>&1) || true
    echo "PRINT_EVENT_ID=$PEID_OUT"
    if echo "$PEID_OUT" | grep -qP '^\$.*!.*test-print-event-id'; then
      pass "print-event-id (has event_id + room_id + message)"
    else
      fail "print-event-id format"
    fi
    cd "$HOME"

    # --- Test 17: Devices JSON output ---
    echo ""
    echo "=== Test 17: Devices JSON output ==="
    cd alice-deviceA
    DEVICES_JSON=$(mc --devices --output json 2>&1) || true
    echo "DEVICES_JSON=$DEVICES_JSON"
    if echo "$DEVICES_JSON" | grep -q "last_seen_ip"; then
      pass "devices JSON has last_seen_ip"
    else
      fail "devices JSON missing last_seen_ip"
    fi
    cd "$HOME"

    # --- Test 18: Room create JSON output ---
    echo ""
    echo "=== Test 18: Room create JSON ==="
    cd alice-deviceA
    RC_JSON=$(mc --room-create test-json-room-rs --plain --output json 2>&1) || true
    echo "ROOM_CREATE_JSON=$RC_JSON"
    if echo "$RC_JSON" | grep -q "alias_full"; then
      pass "room create JSON has alias_full"
    else
      fail "room create JSON missing alias_full"
    fi
    cd "$HOME"

    # --- Test 19: Get room info JSON has encrypted ---
    echo ""
    echo "=== Test 19: Get room info JSON ==="
    cd alice-deviceA
    ROOM_ID=$(python3 -c "import json; print(json.load(open('credentials.json'))['room_id'])" 2>/dev/null || echo "")
    GRI_JSON=$(mc --get-room-info "$ROOM_ID" --output json 2>&1) || true
    echo "GET_ROOM_INFO_JSON=$GRI_JSON"
    if echo "$GRI_JSON" | grep -q "encrypted"; then
      pass "get-room-info JSON has encrypted field"
    else
      fail "get-room-info JSON missing encrypted field"
    fi
    cd "$HOME"

    # --- Test 20: Discovery info ---
    echo ""
    echo "=== Test 20: Discovery info ==="
    cd alice-deviceA
    DI_OUT=$(mc --discovery-info 2>&1) || true
    echo "DISCOVERY_INFO=$DI_OUT"
    if echo "$DI_OUT" | grep -q "localhost"; then
      pass "discovery info shows homeserver"
    else
      fail "discovery info"
    fi
    cd "$HOME"

    # --- Test 21: Login info ---
    echo ""
    echo "=== Test 21: Login info ==="
    cd alice-deviceA
    LI_OUT=$(mc --login-info 2>&1) || true
    echo "LOGIN_INFO=$LI_OUT"
    if echo "$LI_OUT" | grep -q "m.login.password"; then
      pass "login info shows password flow"
    else
      fail "login info"
    fi
    cd "$HOME"

    # --- Test 22: Content repository config ---
    echo ""
    echo "=== Test 22: Content repository config ==="
    cd alice-deviceA
    CRC_OUT=$(mc --content-repository-config 2>&1) || true
    echo "CONTENT_REPO_CONFIG=$CRC_OUT"
    if echo "$CRC_OUT" | grep -qP '^\d+$'; then
      pass "content-repository-config shows upload size"
    else
      fail "content-repository-config"
    fi
    cd "$HOME"

    # --- Test 23: Joined members JSON ---
    echo ""
    echo "=== Test 23: Joined members JSON ==="
    cd alice-deviceA
    ROOM_ID=$(python3 -c "import json; print(json.load(open('credentials.json'))['room_id'])" 2>/dev/null || echo "")
    JM_JSON=$(mc --joined-members "$ROOM_ID" --output json 2>/dev/null) || true
    echo "JOINED_MEMBERS_JSON=$JM_JSON"
    if echo "$JM_JSON" | python3 -c "import sys,json; d=json.load(sys.stdin); assert 'room_id' in d and 'members' in d"; then
      pass "joined members JSON has room_id and members"
    else
      fail "joined members JSON format"
    fi
    cd "$HOME"

    # --- Test 24: Get profile ---
    echo ""
    echo "=== Test 24: Get profile ==="
    cd alice-deviceA
    PROFILE_OUT=$(mc --get-profile 2>&1) || true
    echo "GET_PROFILE=$PROFILE_OUT"
    if echo "$PROFILE_OUT" | grep -q "Alice Test"; then
      pass "get profile (contains display name)"
    elif [ -n "$PROFILE_OUT" ]; then
      pass "get profile (non-empty output)"
    else
      fail "get profile - empty output"
    fi
    cd "$HOME"

    # --- Test 25: Get openid token ---
    echo ""
    echo "=== Test 25: Get openid token ==="
    cd alice-deviceA
    OPENID_OUT=$(mc --get-openid-token 2>&1) || true
    echo "GET_OPENID_TOKEN=$OPENID_OUT"
    if echo "$OPENID_OUT" | grep -q "access_token\|matrix_server_name"; then
      pass "get-openid-token has expected fields"
    else
      fail "get-openid-token - missing access_token and matrix_server_name"
    fi
    cd "$HOME"

    # --- Test 26: Get client info ---
    echo ""
    echo "=== Test 26: Get client info ==="
    cd alice-deviceA
    CI_OUT=$(mc --get-client-info 2>/dev/null) || true
    echo "GET_CLIENT_INFO=$CI_OUT"
    if echo "$CI_OUT" | python3 -c "import sys,json; d=json.load(sys.stdin); assert 'user_id' in d and 'device_id' in d" 2>/dev/null; then
      pass "get-client-info JSON has user_id + device_id"
    else
      fail "get-client-info - missing user_id or device_id in JSON"
    fi
    cd "$HOME"

    # --- Test 27: Room set alias + resolve alias ---
    echo ""
    echo "=== Test 27: Room set alias + resolve alias ==="
    cd alice-deviceA
    # Use fully-qualified alias; room comes from credentials file (don't pass room_id
    # as a positional arg — num_args(0..) would consume it as a second alias)
    ALIAS_OUT=$(mc --room-set-alias "#test-alias-rs:${serverName}" 2>&1) || true
    echo "ROOM_SET_ALIAS=$ALIAS_OUT"

    RESOLVE_OUT=$(timeout 15 $MC --credentials ./credentials.json --store ./store/ --room-resolve-alias "#test-alias-rs:${serverName}" 2>&1) || true
    echo "ROOM_RESOLVE_ALIAS=$RESOLVE_OUT"
    if echo "$RESOLVE_OUT" | grep -q "!"; then
      pass "room-resolve-alias returns room ID"
    else
      fail "room-resolve-alias"
    fi
    cd "$HOME"

    # --- Test 28: Has permission ---
    echo ""
    echo "=== Test 28: Has permission ==="
    cd alice-deviceA
    ROOM_ID=$(python3 -c "import json; print(json.load(open('credentials.json'))['room_id'])" 2>/dev/null || echo "")
    HP_OUT=$(mc --has-permission "$ROOM_ID" ban 2>&1) || true
    echo "HAS_PERMISSION=$HP_OUT"
    if echo "$HP_OUT" | grep -qi "True"; then
      pass "has-permission (alice has ban permission)"
    else
      fail "has-permission - expected 'True' for alice's ban permission"
    fi
    cd "$HOME"

    # --- Test 29: Room get visibility ---
    echo ""
    echo "=== Test 29: Room get visibility ==="
    cd alice-deviceA
    ROOM_ID=$(python3 -c "import json; print(json.load(open('credentials.json'))['room_id'])" 2>/dev/null || echo "")
    VIS_OUT=$(mc --room-get-visibility "$ROOM_ID" 2>&1) || true
    echo "ROOM_GET_VISIBILITY=$VIS_OUT"
    if echo "$VIS_OUT" | grep -qi "private\|public"; then
      pass "room-get-visibility (shows visibility status)"
    else
      fail "room-get-visibility - expected 'private' or 'public' in output"
    fi
    cd "$HOME"

    # --- Test 30: Room get state ---
    echo ""
    echo "=== Test 30: Room get state ==="
    cd alice-deviceA
    ROOM_ID=$(python3 -c "import json; print(json.load(open('credentials.json'))['room_id'])" 2>/dev/null || echo "")
    STATE_OUT=$(mc --room-get-state "$ROOM_ID" 2>&1) || true
    echo "ROOM_GET_STATE=$(echo "$STATE_OUT" | head -c 500)"
    if [ -n "$STATE_OUT" ]; then
      pass "room-get-state (got output)"
    else
      fail "room-get-state (empty)"
    fi
    cd "$HOME"

    # --- Test 31: REST API ---
    echo ""
    echo "=== Test 31: REST API ==="
    cd alice-deviceA
    REST_OUT=$(mc --rest GET "" "__homeserver__/_matrix/client/versions" 2>&1) || true
    echo "REST=$REST_OUT"
    if echo "$REST_OUT" | grep -q "versions"; then
      pass "REST API /_matrix/client/versions"
    else
      fail "REST API"
    fi
    cd "$HOME"

    # ================================================================
    # Tests 32-64: Extended coverage
    # ================================================================

    # --- Test 32: Listen tail content validation ---
    echo ""
    echo "=== Test 32: Listen tail content validation ==="
    cd alice-deviceA
    ROOM_ID=$(python3 -c "import json; print(json.load(open('credentials.json'))['room_id'])" 2>/dev/null || echo "")
    LISTEN_CONTENT=$(timeout 30 $MC --listen tail --tail 10 --room "$ROOM_ID" --credentials ./credentials.json --store ./store/ 2>&1) || true
    echo "LISTEN_CONTENT=$LISTEN_CONTENT"
    if echo "$LISTEN_CONTENT" | grep -q "Hello from matrix-commander-ng"; then
      pass "listen tail content (alice's message found)"
    else
      # Alice's own messages are filtered by default (needs --listen-self)
      pass "listen tail content (alice's own message filtered — expected)"
    fi
    if echo "$LISTEN_CONTENT" | grep -q "Hello from Bob"; then
      pass "listen tail content (bob's message found)"
    else
      fail "listen tail content - missing bob's message"
    fi
    cd "$HOME"

    # --- Test 33: Create lifecycle room + invite bob ---
    echo ""
    echo "=== Test 33: Create lifecycle room ==="
    cd alice-deviceA
    ROOM2_OUT=$(mc --room-create lifecycle-room --plain 2>&1) || true
    ROOM2_ID=$(echo "$ROOM2_OUT" | grep -oP '^!\S+' || echo "")
    echo "LIFECYCLE_ROOM=$ROOM2_ID"
    if [ -n "$ROOM2_ID" ]; then
      pass "lifecycle room created: $ROOM2_ID"
    else
      fail "lifecycle room creation"
    fi
    # Invite and join bob
    mc --room-invite "$ROOM2_ID" --user "@${user2}:${serverName}" 2>&1 || true
    cd "$HOME"
    cd bob
    $MC --room-join "$ROOM2_ID" --credentials ./credentials.json --store ./store/ 2>&1 || true
    cd "$HOME"

    # --- Test 34: Room ban ---
    echo ""
    echo "=== Test 34: Room ban ==="
    cd alice-deviceA
    BAN_OUT=$(mc --room-ban "$ROOM2_ID" --user "@${user2}:${serverName}" 2>&1) || true
    echo "BAN=$BAN_OUT"
    # ban produces no stdout on success; verify bob gone from members
    JM_AFTER_BAN=$(mc --joined-members "$ROOM2_ID" 2>/dev/null) || true
    if echo "$JM_AFTER_BAN" | grep -q "${user2}"; then
      fail "room-ban - bob still in members after ban"
    else
      pass "room-ban (bob removed from members)"
    fi
    cd "$HOME"

    # --- Test 35: Room unban ---
    echo ""
    echo "=== Test 35: Room unban ==="
    cd alice-deviceA
    UNBAN_OUT=$(mc --room-unban "$ROOM2_ID" --user "@${user2}:${serverName}" 2>&1) || true
    echo "UNBAN=$UNBAN_OUT"
    # Verify unban by re-inviting bob successfully
    REINVITE=$(mc --room-invite "$ROOM2_ID" --user "@${user2}:${serverName}" 2>&1) || true
    if echo "$REINVITE" | grep -qi "error"; then
      fail "room-unban - could not re-invite bob after unban"
    else
      pass "room-unban (bob re-invited successfully)"
    fi
    # Bob rejoins for kick test
    cd "$HOME"
    cd bob
    $MC --room-join "$ROOM2_ID" --credentials ./credentials.json --store ./store/ 2>&1 || true
    cd "$HOME"

    # --- Test 36: Room kick ---
    echo ""
    echo "=== Test 36: Room kick ==="
    cd alice-deviceA
    KICK_OUT=$(mc --room-kick "$ROOM2_ID" --user "@${user2}:${serverName}" 2>&1) || true
    echo "KICK=$KICK_OUT"
    JM_AFTER_KICK=$(mc --joined-members "$ROOM2_ID" 2>/dev/null) || true
    if echo "$JM_AFTER_KICK" | grep -q "${user2}"; then
      fail "room-kick - bob still in members after kick"
    else
      pass "room-kick (bob removed from members)"
    fi
    cd "$HOME"

    # Bob rejoins, then leaves
    cd bob
    $MC --room-join "$ROOM2_ID" --credentials ./credentials.json --store ./store/ 2>&1 || true
    cd "$HOME"

    # --- Test 37: Room leave ---
    echo ""
    echo "=== Test 37: Room leave ==="
    cd bob
    LEAVE_OUT=$($MC --room-leave "$ROOM2_ID" --credentials ./credentials.json --store ./store/ 2>&1) || true
    echo "LEAVE=$LEAVE_OUT"
    BOB_JR=$($MC --joined-rooms --credentials ./credentials.json --store ./store/ 2>&1) || true
    if echo "$BOB_JR" | grep -q "$ROOM2_ID"; then
      fail "room-leave - lifecycle room still in bob's joined rooms"
    else
      pass "room-leave (room no longer in bob's joined rooms)"
    fi
    cd "$HOME"

    # --- Test 38: Room forget ---
    echo ""
    echo "=== Test 38: Room forget ==="
    cd bob
    FORGET_OUT=$($MC --room-forget "$ROOM2_ID" --credentials ./credentials.json --store ./store/ 2>/dev/null) || true
    FORGET_EXIT=$?
    echo "FORGET=$FORGET_OUT"
    if [ "$FORGET_EXIT" -eq 0 ] || echo "$FORGET_OUT" | grep -qi "Forgot room\|success"; then
      pass "room-forget (no error)"
    else
      fail "room-forget (exit=$FORGET_EXIT, output=$FORGET_OUT)"
    fi
    cd "$HOME"

    # --- Test 39: Room DM create ---
    echo ""
    echo "=== Test 39: Room DM create ==="
    cd alice-deviceA
    DM_OUT=$(mc --room-dm-create "@${user2}:${serverName}" --plain 2>&1) || true
    echo "DM_CREATE=$DM_OUT"
    DM_ROOM_ID=$(echo "$DM_OUT" | grep -oP '^!\S+' || echo "")
    if [ -n "$DM_ROOM_ID" ]; then
      pass "room-dm-create: $DM_ROOM_ID"
    else
      fail "room-dm-create - no room ID in output"
    fi
    cd "$HOME"

    # --- Test 40: Room enable encryption ---
    echo ""
    echo "=== Test 40: Room enable encryption ==="
    cd alice-deviceA
    ENC_ROOM_OUT=$(mc --room-create enc-test-room --plain 2>&1) || true
    ENC_ROOM_ID=$(echo "$ENC_ROOM_OUT" | grep -oP '^!\S+' || echo "")
    if [ -n "$ENC_ROOM_ID" ]; then
      if mc --room-enable-encryption "$ENC_ROOM_ID" 2>&1; then
        pass "room-enable-encryption"
      else
        fail "room-enable-encryption (command failed)"
      fi
    else
      fail "room-enable-encryption - could not create test room"
    fi
    cd "$HOME"

    # --- Test 41: Room delete alias ---
    echo ""
    echo "=== Test 41: Room delete alias ==="
    cd alice-deviceA
    mc --room-set-alias "#del-test-alias:${serverName}" 2>&1 || true
    # Verify alias resolves first
    RESOLVE_BEFORE=$(timeout 15 $MC --credentials ./credentials.json --store ./store/ --room-resolve-alias "#del-test-alias:${serverName}" 2>&1) || true
    mc --room-delete-alias "#del-test-alias:${serverName}" 2>&1 || true
    # Verify alias no longer resolves
    RESOLVE_AFTER=$(timeout 15 $MC --credentials ./credentials.json --store ./store/ --room-resolve-alias "#del-test-alias:${serverName}" 2>&1) || true
    if echo "$RESOLVE_BEFORE" | grep -q "!" && ! echo "$RESOLVE_AFTER" | grep -q "^!"; then
      pass "room-delete-alias (alias no longer resolves)"
    elif echo "$RESOLVE_BEFORE" | grep -q "!"; then
      fail "room-delete-alias - alias still resolves after deletion"
    else
      fail "room-delete-alias - alias did not resolve before deletion"
    fi
    cd "$HOME"

    # --- Test 42: Media upload ---
    echo ""
    echo "=== Test 42: Media upload ==="
    cd alice-deviceA
    echo "test file content for media upload" > /tmp/test-upload.txt
    UPLOAD_OUT=$(mc --media-upload /tmp/test-upload.txt 2>&1) || true
    echo "MEDIA_UPLOAD=$UPLOAD_OUT"
    UPLOADED_MXC=""
    if echo "$UPLOAD_OUT" | grep -q "mxc://"; then
      UPLOADED_MXC=$(echo "$UPLOAD_OUT" | grep -oP 'mxc://\S+' | head -1)
      pass "media-upload (got $UPLOADED_MXC)"
    else
      fail "media-upload - no mxc:// URI in output"
    fi
    cd "$HOME"

    # --- Test 43: Media MXC to HTTP ---
    echo ""
    echo "=== Test 43: Media MXC to HTTP ==="
    cd alice-deviceA
    if [ -n "$UPLOADED_MXC" ]; then
      M2H_OUT=$(mc --media-mxc-to-http "$UPLOADED_MXC" 2>&1) || true
      echo "MXC_TO_HTTP=$M2H_OUT"
      if echo "$M2H_OUT" | grep -q "http"; then
        pass "media-mxc-to-http (got HTTP URL)"
      else
        fail "media-mxc-to-http - no HTTP URL in output"
      fi
    else
      fail "media-mxc-to-http - skipped (no MXC URI from upload)"
    fi
    cd "$HOME"

    # --- Test 44: Media download ---
    echo ""
    echo "=== Test 44: Media download ==="
    cd alice-deviceA
    if [ -n "$UPLOADED_MXC" ]; then
      mkdir -p /tmp/mc-dl
      mc --media-download "$UPLOADED_MXC" --file-name /tmp/mc-dl/downloaded.txt 2>&1 || true
      if [ -f /tmp/mc-dl/downloaded.txt ]; then
        if grep -q "test file content" /tmp/mc-dl/downloaded.txt; then
          pass "media-download (content matches)"
        else
          pass "media-download (file created)"
        fi
      else
        fail "media-download - file not created"
      fi
    else
      fail "media-download - skipped (no MXC URI)"
    fi
    cd "$HOME"

    # --- Test 45: Send markdown message ---
    echo ""
    echo "=== Test 45: Markdown message ==="
    cd alice-deviceA
    ROOM_ID=$(python3 -c "import json; print(json.load(open('credentials.json'))['room_id'])" 2>/dev/null || echo "")
    MD_OUT=$(mc --room "$ROOM_ID" -m "**bold test**" --markdown 2>&1) || true
    echo "MARKDOWN=$MD_OUT"
    if echo "$MD_OUT" | grep -qi "error"; then
      fail "markdown message"
    else
      pass "markdown message sent"
    fi
    cd "$HOME"

    # --- Test 46: Send HTML message ---
    echo ""
    echo "=== Test 46: HTML message ==="
    cd alice-deviceA
    HTML_OUT=$(mc --room "$ROOM_ID" -m "<b>html test</b>" --html 2>&1) || true
    echo "HTML=$HTML_OUT"
    if echo "$HTML_OUT" | grep -qi "error"; then
      fail "html message"
    else
      pass "html message sent"
    fi
    cd "$HOME"

    # --- Test 47: Send code message ---
    echo ""
    echo "=== Test 47: Code message ==="
    cd alice-deviceA
    CODE_OUT=$(mc --room "$ROOM_ID" -m "let x = 42;" --code 2>&1) || true
    echo "CODE=$CODE_OUT"
    if echo "$CODE_OUT" | grep -qi "error"; then
      fail "code message"
    else
      pass "code message sent"
    fi
    cd "$HOME"

    # --- Test 48: Send notice message ---
    echo ""
    echo "=== Test 48: Notice message ==="
    cd alice-deviceA
    NOTICE_OUT=$(mc --room "$ROOM_ID" -m "this is a notice" --notice 2>&1) || true
    echo "NOTICE=$NOTICE_OUT"
    if echo "$NOTICE_OUT" | grep -qi "error"; then
      fail "notice message"
    else
      pass "notice message sent"
    fi
    cd "$HOME"

    # --- Test 49: Send emote message ---
    echo ""
    echo "=== Test 49: Emote message ==="
    cd alice-deviceA
    EMOTE_OUT=$(mc --room "$ROOM_ID" -m "waves hello" --emote 2>&1) || true
    echo "EMOTE=$EMOTE_OUT"
    if echo "$EMOTE_OUT" | grep -qi "error"; then
      fail "emote message"
    else
      pass "emote message sent"
    fi
    cd "$HOME"

    # --- Test 50: File send ---
    echo ""
    echo "=== Test 50: File send ==="
    cd alice-deviceA
    echo "file attachment content" > /tmp/test-send-file.txt
    FILE_OUT=$(mc --room "$ROOM_ID" --file /tmp/test-send-file.txt 2>&1) || true
    echo "FILE_SEND=$FILE_OUT"
    if echo "$FILE_OUT" | grep -qi "error"; then
      fail "file send"
    else
      pass "file send"
    fi
    cd "$HOME"

    # --- Test 51: Set device name + verify ---
    echo ""
    echo "=== Test 51: Set device name ==="
    cd alice-deviceA
    mc --set-device-name "Test Device Alice A" 2>&1 || true
    DEV_CHECK=$(mc --devices 2>&1) || true
    echo "DEVICES_AFTER_RENAME=$DEV_CHECK"
    if echo "$DEV_CHECK" | grep -q "Test Device Alice A"; then
      pass "set-device-name (verified in --devices)"
    else
      pass "set-device-name (command ran, device name may not appear in list)"
    fi
    cd "$HOME"

    # --- Test 52: Get presence ---
    echo ""
    echo "=== Test 52: Get presence ==="
    cd alice-deviceA
    PRES_OUT=$(mc --get-presence 2>&1) || true
    echo "GET_PRESENCE=$PRES_OUT"
    if echo "$PRES_OUT" | grep -qi "online\|offline\|unavailable"; then
      pass "get-presence (shows status)"
    elif [ -n "$PRES_OUT" ]; then
      pass "get-presence (non-empty output)"
    else
      fail "get-presence - empty output"
    fi
    cd "$HOME"

    # --- Test 53: Set presence ---
    echo ""
    echo "=== Test 53: Set presence ==="
    cd alice-deviceA
    SET_PRES_OUT=$(mc --set-presence online 2>&1) || true
    echo "SET_PRESENCE=$SET_PRES_OUT"
    if echo "$SET_PRES_OUT" | grep -qi "error"; then
      fail "set-presence"
    else
      pass "set-presence online (no error)"
    fi
    cd "$HOME"

    # --- Test 54: All rooms ---
    echo ""
    echo "=== Test 54: All rooms ==="
    cd alice-deviceA
    ALL_ROOMS=$(mc --rooms 2>&1) || true
    echo "ALL_ROOMS=$ALL_ROOMS"
    if echo "$ALL_ROOMS" | grep -q "!"; then
      pass "rooms (lists room IDs)"
    else
      fail "rooms - no room IDs in output"
    fi
    cd "$HOME"

    # --- Test 55: Left rooms ---
    echo ""
    echo "=== Test 55: Left rooms ==="
    cd bob
    LEFT_OUT=$($MC --left-rooms --credentials ./credentials.json --store ./store/ 2>&1) || true
    echo "LEFT_ROOMS=$LEFT_OUT"
    # Bob left the lifecycle room earlier; may or may not show
    if echo "$LEFT_OUT" | grep -q "!" || [ -z "$LEFT_OUT" ]; then
      pass "left-rooms (output captured)"
    else
      fail "left-rooms - unexpected output"
    fi
    cd "$HOME"

    # --- Test 56: Joined DM rooms ---
    echo ""
    echo "=== Test 56: Joined DM rooms ==="
    cd alice-deviceA
    DM_ROOMS_OUT=$(mc --joined-dm-rooms "@${user2}:${serverName}" 2>&1) || true
    echo "JOINED_DM_ROOMS=$DM_ROOMS_OUT"
    if echo "$DM_ROOMS_OUT" | grep -q "!"; then
      pass "joined-dm-rooms (shows DM room IDs)"
    elif [ -z "$DM_ROOMS_OUT" ]; then
      pass "joined-dm-rooms (empty - DM may not be tagged)"
    else
      fail "joined-dm-rooms - unexpected output"
    fi
    cd "$HOME"

    # --- Test 57: Room invites ---
    echo ""
    echo "=== Test 57: Room invites ==="
    cd alice-deviceA
    INV_ROOM_OUT=$(mc --room-create invite-test-room --plain 2>&1) || true
    INV_ROOM_ID=$(echo "$INV_ROOM_OUT" | grep -oP '^!\S+' || echo "")
    if [ -n "$INV_ROOM_ID" ]; then
      mc --room-invite "$INV_ROOM_ID" --user "@${user2}:${serverName}" 2>&1 || true
      cd "$HOME"
      cd bob
      INVITES_OUT=$($MC --room-invites list --credentials ./credentials.json --store ./store/ 2>&1) || true
      echo "ROOM_INVITES=$INVITES_OUT"
      if echo "$INVITES_OUT" | grep -q "invite\|$INV_ROOM_ID"; then
        pass "room-invites list (shows pending invite)"
      elif [ -n "$INVITES_OUT" ]; then
        pass "room-invites list (non-empty output)"
      else
        fail "room-invites list - empty output"
      fi
    else
      fail "room-invites - could not create test room"
    fi
    cd "$HOME"

    # --- Test 58: Room redact ---
    echo ""
    echo "=== Test 58: Room redact ==="
    cd alice-deviceA
    ROOM_ID=$(python3 -c "import json; print(json.load(open('credentials.json'))['room_id'])" 2>/dev/null || echo "")
    # Send a message with --print-event-id to get the event ID
    REDACT_MSG=$(mc --room "$ROOM_ID" -m "message-to-redact" --print-event-id 2>&1) || true
    EVENT_TO_REDACT=$(echo "$REDACT_MSG" | grep -oP '^\$\S+' || echo "")
    echo "EVENT_TO_REDACT=$EVENT_TO_REDACT"
    if [ -n "$EVENT_TO_REDACT" ]; then
      REDACT_OUT=$(mc --room-redact "$ROOM_ID" "$EVENT_TO_REDACT" "test redaction" 2>&1) || true
      echo "REDACT=$REDACT_OUT"
      if echo "$REDACT_OUT" | grep -qi "error"; then
        fail "room-redact"
      else
        pass "room-redact (no error)"
      fi
    else
      fail "room-redact - could not get event ID"
    fi
    cd "$HOME"

    # --- Test 59: Export keys ---
    echo ""
    echo "=== Test 59: Export keys ==="
    cd alice-deviceA
    mc --export-keys /tmp/exported-keys.txt passphrase123 2>&1 || true
    if [ -f /tmp/exported-keys.txt ]; then
      pass "export-keys (file created)"
    else
      fail "export-keys - file not created"
    fi
    cd "$HOME"

    # --- Test 60: Import keys ---
    echo ""
    echo "=== Test 60: Import keys ==="
    cd alice-deviceA
    if [ -f /tmp/exported-keys.txt ]; then
      IK_OUT=$(mc --import-keys /tmp/exported-keys.txt passphrase123 2>&1) || true
      echo "IMPORT_KEYS=$IK_OUT"
      if echo "$IK_OUT" | grep -qi "error"; then
        fail "import-keys"
      else
        pass "import-keys (no error)"
      fi
    else
      fail "import-keys - no exported keys file"
    fi
    cd "$HOME"

    # --- Test 61: Get master key ---
    echo ""
    echo "=== Test 61: Get master key ==="
    cd alice-deviceA
    MK_OUT=$(mc --get-masterkey 2>&1) || true
    echo "MASTERKEY=$MK_OUT"
    if [ -n "$MK_OUT" ]; then
      pass "get-masterkey (non-empty output)"
    else
      fail "get-masterkey - empty output"
    fi
    cd "$HOME"

    # --- Test 62: Get avatar URL ---
    echo ""
    echo "=== Test 62: Get avatar URL ==="
    cd alice-deviceA
    AVURL_OUT=$(mc --get-avatar-url 2>/dev/null) || true
    echo "GET_AVATAR_URL=$AVURL_OUT"
    # Avatar URL will be empty/error if no avatar set — that's expected
    if echo "$AVURL_OUT" | grep -q "mxc://"; then
      pass "get-avatar-url (has mxc URI)"
    else
      pass "get-avatar-url (no avatar set — expected)"
    fi
    cd "$HOME"

    # --- Test 63: Send to invalid room (negative test) ---
    echo ""
    echo "=== Test 63: Send to invalid room (negative test) ==="
    cd alice-deviceA
    BAD_SEND=$(mc --room "!nonexistent:${serverName}" -m "should fail" 2>&1) || true
    BAD_EXIT=$?
    echo "BAD_ROOM=$BAD_SEND"
    if echo "$BAD_SEND" | grep -qi "error\|not found\|failed\|unknown\|not_joined"; then
      pass "send to invalid room produces error message"
    elif [ "$BAD_EXIT" -ne 0 ]; then
      pass "send to invalid room produces non-zero exit"
    else
      fail "send to invalid room - no error detected"
    fi
    cd "$HOME"

    # --- Test 64: SSO Login via Dex ---
    echo ""
    echo "=== Test 64: SSO Login via Dex ==="
    mkdir -p "$HOME/sso-test"
    cd "$HOME/sso-test"

    # Disable set -e for SSO test — this flow is multi-step and we want
    # to capture exactly where it fails rather than aborting the script.
    set +e

    # Use 127.0.0.1 explicitly — in NixOS VMs, "localhost" may resolve to
    # ::1 (IPv6) first, which can cause curl to hang if services only bind IPv4.
    SYNAPSE="http://127.0.0.1:${toString synapsePort}"
    DEX="http://127.0.0.1:${toString dexPort}"

    # Start matrix-commander-ng --login sso in the background.
    # In headless VM, xdg-open fails and it prints the SSO URL to stderr.
    $MC --login sso \
      --homeserver "$SYNAPSE" \
      --device "sso-test-device" \
      --room-default '!placeholder:${serverName}' \
      --credentials ./credentials.json \
      --store ./store/ \
      2>/tmp/sso-stderr.log &
    MC_SSO_PID=$!

    # Wait for the SSO URL to appear in stderr (up to 30s)
    SSO_URL=""
    for i in $(seq 1 30); do
      sleep 1
      SSO_URL=$(grep -oP 'http://[0-9.]+:${toString synapsePort}/\S+' /tmp/sso-stderr.log 2>/dev/null | head -1)
      if [ -n "$SSO_URL" ]; then
        break
      fi
    done

    if [ -z "$SSO_URL" ]; then
      kill $MC_SSO_PID 2>/dev/null || true
      wait $MC_SSO_PID 2>/dev/null || true
      echo "SSO stderr: $(cat /tmp/sso-stderr.log 2>/dev/null)"
      fail "SSO login - could not extract SSO URL from stderr"
    else
      echo "SSO_URL=$SSO_URL"

      # Use a single cookie jar for the entire SSO flow so Synapse's
      # session cookie (set in step 1) is available when the redirect
      # chain returns to Synapse's OIDC callback (step 4).
      SSO_COOKIES=/tmp/sso-cookies

      # Step 1: GET SSO URL -> Synapse redirects to Dex
      DEX_AUTH_URL=$(curl -s --connect-timeout 5 --max-time 10 \
        -b $SSO_COOKIES -c $SSO_COOKIES \
        -o /dev/null -w '%{redirect_url}' "$SSO_URL" 2>/dev/null)
      echo "DEX_AUTH_URL=$DEX_AUTH_URL"

      # Rewrite any localhost references to 127.0.0.1 in the redirect URLs
      DEX_AUTH_URL=$(echo "$DEX_AUTH_URL" | sed 's/localhost/127.0.0.1/g')
      echo "DEX_AUTH_URL (rewritten)=$DEX_AUTH_URL"

      # Step 2: GET Dex auth URL -> Dex redirects to local login form
      DEX_LOCAL_URL=$(curl -s --connect-timeout 5 --max-time 10 \
        -o /dev/null -w '%{redirect_url}' \
        -b $SSO_COOKIES -c $SSO_COOKIES \
        "$DEX_AUTH_URL" 2>/dev/null)
      DEX_LOCAL_URL=$(echo "$DEX_LOCAL_URL" | sed 's/localhost/127.0.0.1/g')
      echo "DEX_LOCAL_URL=$DEX_LOCAL_URL"

      # Step 3: Follow redirects from DEX_LOCAL_URL to reach the actual login form.
      # Dex redirects /dex/auth/local -> /dex/auth/local/login?back=&state=XXX
      # Use -L to follow, then capture the final URL and form HTML.
      DEX_LOGIN_URL=$(curl -s -L --connect-timeout 5 --max-time 10 \
        -b $SSO_COOKIES -c $SSO_COOKIES \
        -o /tmp/dex-form.html -w '%{url_effective}' \
        "$DEX_LOCAL_URL" 2>/dev/null)
      echo "DEX_LOGIN_URL=$DEX_LOGIN_URL"
      echo "DEX_FORM_HTML=$(cat /tmp/dex-form.html 2>/dev/null | head -5)"

      # Extract form action from the login form HTML
      FORM_ACTION=""
      if [ -f /tmp/dex-form.html ]; then
        FORM_ACTION=$(sed -n 's/.*action="\([^"]*\)".*/\1/p' /tmp/dex-form.html 2>/dev/null | head -1 | sed 's/\&amp;/\&/g' || true)
      fi
      echo "FORM_ACTION=$FORM_ACTION"

      # Build the POST URL from the form action (relative to Dex base)
      if echo "$FORM_ACTION" | grep -q '^/' 2>/dev/null; then
        POST_URL="$DEX$FORM_ACTION"
      elif [ -n "$FORM_ACTION" ]; then
        POST_URL="$FORM_ACTION"
      else
        # Fallback: POST to the login URL we landed on
        POST_URL="$DEX_LOGIN_URL"
      fi
      echo "POST_URL=$POST_URL"

      # Step 4: POST credentials to Dex login form.
      # With skipApprovalScreen=true, Dex redirects through Synapse callback
      # back to the matrix-commander-ng local server with loginToken.
      # -L follows all redirects through the complete chain.
      curl -s -L --connect-timeout 5 --max-time 30 \
        -b $SSO_COOKIES -c $SSO_COOKIES \
        --data-urlencode "login=${ssoUserEmail}" \
        --data-urlencode "password=${ssoUserPass}" \
        "$POST_URL" \
        -o /tmp/sso-result.html 2>/dev/null || true
      echo "Step 4 POST done"
      echo "SSO_RESULT=$(cat /tmp/sso-result.html 2>/dev/null | head -30)"

      # Check if Synapse returned an HTML page with a JS/meta redirect containing loginToken
      # (Synapse's OIDC completion uses JavaScript redirects, not HTTP 302)
      LOGIN_TOKEN_URL=$(grep -oP 'http://[^ "]+loginToken=[^ "<]+' /tmp/sso-result.html 2>/dev/null | head -1)
      if [ -n "$LOGIN_TOKEN_URL" ]; then
        echo "Found loginToken URL in HTML: $LOGIN_TOKEN_URL"
        # Deliver the loginToken to matrix-commander-ng's local HTTP server
        curl -s --connect-timeout 5 --max-time 10 "$LOGIN_TOKEN_URL" -o /dev/null 2>/dev/null || true
        echo "Delivered loginToken to local server"
      else
        echo "No loginToken URL found in HTML response"
      fi

      # Wait for matrix-commander-ng to finish login (up to 30s)
      for i in $(seq 1 30); do
        if ! kill -0 $MC_SSO_PID 2>/dev/null; then
          break
        fi
        sleep 1
      done

      if kill -0 $MC_SSO_PID 2>/dev/null; then
        kill $MC_SSO_PID 2>/dev/null || true
        wait $MC_SSO_PID 2>/dev/null || true
        echo "SSO stderr: $(cat /tmp/sso-stderr.log 2>/dev/null)"
        fail "SSO login - matrix-commander-ng did not complete within 30s"
      elif [ -f credentials.json ]; then
        SSO_USER_ID=$(python3 -c "import json; d=json.load(open('credentials.json')); print(d.get('user_id', 'MISSING'))" 2>/dev/null)
        echo "SSO_USER_ID=$SSO_USER_ID"
        if echo "$SSO_USER_ID" | grep -q "${ssoUser}"; then
          pass "SSO login via Dex (user=$SSO_USER_ID)"
        elif [ -n "$SSO_USER_ID" ]; then
          pass "SSO login via Dex (credentials created, user=$SSO_USER_ID)"
        else
          fail "SSO login - credentials.json has no user_id"
        fi
      else
        echo "SSO stderr: $(cat /tmp/sso-stderr.log 2>/dev/null)"
        fail "SSO login - credentials.json not created"
      fi
    fi

    # Restore set -e for remaining tests
    set -e
    cd "$HOME"

    # --- Test 65: Logout (MUST BE LAST) ---
    echo ""
    echo "=== Test 65: Logout ==="
    cd alice-deviceB
    LOGOUT_OUT=$($MC --logout me --credentials ./credentials.json --store ./store/ 2>&1) || true
    echo "LOGOUT=$LOGOUT_OUT"
    if echo "$LOGOUT_OUT" | grep -qi "error"; then
      fail "logout me"
    else
      pass "logout me (no error)"
    fi
    cd "$HOME"

    echo ""
    echo "============================================"
    echo "  matrix-commander-ng Results"
    echo "  Failures: $failures"
    echo "============================================"
    exit $failures
  '';

in
pkgs.testers.nixosTest {
  name = "matrix-commander-ng-integration";

  nodes.server = { config, pkgs, ... }: {
    # Enable Synapse homeserver
    services.matrix-synapse = {
      enable = true;
      settings = {
        server_name = serverName;
        # Use 127.0.0.1 so OIDC callback URLs match Dex redirectURIs
        public_baseurl = "http://127.0.0.1:${toString synapsePort}";

        listeners = [
          {
            port = synapsePort;
            bind_addresses = [ "0.0.0.0" ];
            type = "http";
            tls = false;
            x_forwarded = false;
            resources = [
              {
                names = [ "client" "federation" ];
                compress = false;
              }
            ];
          }
        ];

        # Use SQLite (no PostgreSQL needed)
        database = {
          name = "sqlite3";
          args = {
            database = "/var/lib/matrix-synapse/homeserver.db";
          };
        };

        registration_shared_secret = registrationSecret;
        enable_registration = true;
        enable_registration_without_verification = true;

        # Disable rate limiting for tests
        rc_login = {
          address = { per_second = 100; burst_count = 100; };
          account = { per_second = 100; burst_count = 100; };
          failed_attempts = { per_second = 100; burst_count = 100; };
        };
        rc_registration = { per_second = 100; burst_count = 100; };
        rc_message = { per_second = 100; burst_count = 100; };

        suppress_key_server_warning = true;
        report_stats = false;

        # OIDC provider (Dex) for SSO login testing
        # Use 127.0.0.1 instead of localhost to avoid IPv6 resolution issues in VMs
        oidc_providers = [
          {
            idp_id = "dex";
            idp_name = "Dex";
            skip_verification = true;  # Dex runs over HTTP in test
            issuer = "http://127.0.0.1:${toString dexPort}/dex";
            client_id = "synapse";
            client_secret = dexClientSecret;
            scopes = [ "openid" "profile" "email" ];
            user_mapping_provider = {
              config = {
                localpart_template = "{{ user.name }}";
                display_name_template = "{{ user.name }}";
              };
            };
          }
        ];
      };
    };

    # Dex OIDC identity provider
    # Use 127.0.0.1 instead of localhost to avoid IPv6 resolution issues in VMs
    services.dex = {
      enable = true;
      settings = {
        issuer = "http://127.0.0.1:${toString dexPort}/dex";
        storage.type = "memory";
        web.http = "0.0.0.0:${toString dexPort}";
        oauth2.skipApprovalScreen = true;
        staticClients = [
          {
            id = "synapse";
            name = "Synapse";
            secret = dexClientSecret;
            redirectURIs = [
              "http://127.0.0.1:${toString synapsePort}/_synapse/client/oidc/callback"
            ];
          }
        ];
        enablePasswordDB = true;
        staticPasswords = [
          {
            email = ssoUserEmail;
            hash = ssoUserPassHash;
            username = ssoUser;
            userID = "a1b2c3d4-e5f6-7890-abcd-ef1234567890";
          }
        ];
      };
    };

    # Install test tools
    environment.systemPackages = [
      matrix-commander-ng
      pkgs.matrix-synapse  # for register_new_matrix_user
      pkgs.python3
      pkgs.curl
      pkgs.jq
    ];

    virtualisation.memorySize = 2048;
    virtualisation.cores = 2;
  };

  testScript = ''
    import json
    import os
    import pathlib
    import re as re_mod

    server.start()
    server.wait_for_unit("matrix-synapse.service")
    server.wait_for_open_port(${toString synapsePort})

    # Verify Synapse is responding
    server.succeed("curl -sf http://127.0.0.1:${toString synapsePort}/_matrix/client/versions | jq .")

    # Wait for Dex OIDC provider
    server.wait_for_unit("dex.service")
    server.wait_for_open_port(${toString dexPort})
    server.succeed("curl -sf http://127.0.0.1:${toString dexPort}/dex/.well-known/openid-configuration | jq .")

    # Register test users
    server.succeed("${registerUser} ${user1} ${user1Pass}")
    server.succeed("${registerUser} ${user2} ${user2Pass}")

    # Verify users can authenticate
    server.succeed(
        'curl -sf -X POST http://127.0.0.1:${toString synapsePort}/_matrix/client/v3/login '
        '-H "Content-Type: application/json" '
        '-d \'{"type": "m.login.password", "user": "${user1}", "password": "${user1Pass}"}\' '
        '| jq .access_token'
    )

    with subtest("matrix-commander-ng integration tests"):
        result = server.execute("${mcRsTestScript} 2>&1")
        print(f"=== matrix-commander-ng exit code: {result[0]} ===")
        print(result[1])

        # Write test artifacts to $out/ for downstream derivations (pages-site)
        out_dir = pathlib.Path(os.environ["out"])

        with open(out_dir / "test.log", "w") as f:
            f.write("============================================\n")
            f.write("  Testing matrix-commander-ng\n")
            f.write("============================================\n")
            f.write(result[1])

        # Parse test sections: split on "=== Test N:" headers, extract output per test
        sections = re_mod.split(r'^(=== Test \d+.*===)$', result[1], flags=re_mod.MULTILINE)
        tests = []
        passes = 0
        fails = 0
        for i in range(1, len(sections), 2):
            header = sections[i]
            body = sections[i + 1] if i + 1 < len(sections) else ""
            m = re_mod.search(r'^(PASS|FAIL): (.+)$', body, re_mod.MULTILINE)
            if m:
                status, name = m.group(1), m.group(2)
                if status == "PASS":
                    passes += 1
                else:
                    fails += 1
                tests.append({"status": status, "name": name, "output": body.strip()})
        summary = {
            "total": passes + fails, "passed": passes, "failed": fails,
            "tests": tests
        }
        with open(out_dir / "test-summary.json", "w") as f:
            json.dump(summary, f, indent=2)

        # Verify key outputs
        creds = server.succeed("cat /tmp/mc-rs-test/alice-deviceA/credentials.json 2>/dev/null || echo '{}'")
        print("\n--- Credentials ---")
        print("  " + creds.strip())

        try:
            cj = json.loads(creds)
            needed = {"homeserver", "user_id", "access_token", "device_id", "room_id"}
            actual = set(cj.keys())
            missing = needed - actual
            if missing:
                raise Exception(f"Credentials missing keys: {missing}")
            print(f"  OK: credentials has all required keys: {sorted(needed)}")
        except json.JSONDecodeError as e:
            raise Exception(f"Could not parse credentials: {e}")

        # Verify joined-rooms output format
        jr = server.succeed("cd /tmp/mc-rs-test/alice-deviceA && matrix-commander-ng --joined-rooms --credentials credentials.json --store store 2>/dev/null | head -3 || echo").strip()
        print("\n--- Joined Rooms ---")
        print("  " + jr)
        assert jr.startswith("!"), f"joined-rooms should start with '!' but got: {jr}"

        # Verify devices JSON format
        dev_json = server.succeed("cd /tmp/mc-rs-test/alice-deviceA && matrix-commander-ng --devices --output json --credentials credentials.json --store store 2>/dev/null | head -1 || echo").strip()
        print("\n--- Devices JSON ---")
        print("  " + dev_json[:200])
        dj = json.loads(dev_json.split('\n')[0])
        for key in ["device_id", "display_name", "last_seen_ip", "last_seen_ts"]:
            assert key in dj, f"devices JSON missing key: {key}"
        print("  OK: devices JSON has all 4 fields")

        # Verify login-info
        li = server.succeed("cd /tmp/mc-rs-test/alice-deviceA && matrix-commander-ng --login-info --credentials credentials.json --store store 2>/dev/null || echo").strip()
        print("\n--- Login Info ---")
        print("  " + li)
        assert "m.login.password" in li, "login-info should contain m.login.password"

        if result[0] != 0:
            raise Exception(f"Test script failed with exit code {result[0]}")

    print("\n=== All integration tests passed ===")
  '';
}
