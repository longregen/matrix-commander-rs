{ pkgs, lib, matrix-commander-ng-local, ... }:

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
      http://localhost:${toString synapsePort}
    echo "Registered user $USERNAME"
  '';

  # ============================================================
  # Python matrix-commander test script
  # ============================================================
  mcPyTestScript = pkgs.writeShellScript "test-matrix-commander-py" ''
    set -euo pipefail
    export HOME=/tmp/mc-py-test
    mkdir -p "$HOME/outputs"
    cd "$HOME"

    MC="matrix-commander"
    SERVER="http://localhost:${toString synapsePort}"

    failures=0
    fail() { echo "FAIL: $1"; failures=$((failures + 1)); }
    pass() { echo "PASS: $1"; }
    skip() { echo "SKIP: $1"; }

    # Save command output to a file.
    # Usage: capture <name> <format> <mc-args...>
    # format: "text" or "json" (appends --output json)
    capture() {
      local name="$1" fmt="$2"; shift 2
      local outfile="$HOME/outputs/$name-$fmt.out"
      if [ "$fmt" = "json" ]; then
        $MC "$@" --output json 2>/dev/null > "$outfile" || true
      else
        $MC "$@" 2>/dev/null > "$outfile" || true
      fi
    }

    echo "============================================"
    echo "  Testing matrix-commander (Python)"
    echo "============================================"

    # ---- Phase 1: Login ----
    echo ""
    echo "=== Login alice device A ==="
    mkdir -p alice-deviceA && cd alice-deviceA
    $MC --login password \
      --homeserver "$SERVER" \
      --user-login "@${user1}:${serverName}" \
      --password "${user1Pass}" \
      --device "alice-deviceA-py" \
      --room-default '!placeholder:${serverName}' 2>&1 || true
    [ -f credentials.json ] && pass "alice deviceA login" || fail "alice deviceA login"
    cd "$HOME"

    echo ""
    echo "=== Login alice device B ==="
    mkdir -p alice-deviceB && cd alice-deviceB
    $MC --login password \
      --homeserver "$SERVER" \
      --user-login "@${user1}:${serverName}" \
      --password "${user1Pass}" \
      --device "alice-deviceB-py" \
      --room-default '!placeholder:${serverName}' 2>&1 || true
    [ -f credentials.json ] && pass "alice deviceB login" || fail "alice deviceB login"
    cd "$HOME"

    echo ""
    echo "=== Login bob ==="
    mkdir -p bob && cd bob
    $MC --login password \
      --homeserver "$SERVER" \
      --user-login "@${user2}:${serverName}" \
      --password "${user2Pass}" \
      --device "bob-device-py" \
      --room-default '!placeholder:${serverName}' 2>&1 || true
    [ -f credentials.json ] && pass "bob login" || fail "bob login"
    cd "$HOME"

    # ---- Phase 2: Setup ----
    cd alice-deviceA

    echo ""
    echo "=== Set display name ==="
    if $MC --set-display-name "Alice Test" 2>&1; then
      pass "set display name"
    else
      fail "set display name"
    fi

    echo ""
    echo "=== Create room ==="
    ROOM_OUT=$($MC --room-create test-room-py --plain 2>&1) || true
    echo "PY_ROOM_CREATE=$ROOM_OUT"
    ROOM_ID=$(echo "$ROOM_OUT" | grep -oP 'room id "\K[^"]+' || echo "")
    if [ -n "$ROOM_ID" ]; then
      pass "create room - $ROOM_ID"
      python3 -c "
import json
c = json.load(open('credentials.json'))
c['room_id'] = '$ROOM_ID'
json.dump(c, open('credentials.json', 'w'))
"
    else
      fail "create room"
    fi

    echo ""
    echo "=== Invite bob and join ==="
    if $MC --room-invite "$ROOM_ID" --user "@${user2}:${serverName}" 2>&1; then
      pass "invite bob"
    else
      fail "invite bob"
    fi
    cd "$HOME/bob"
    if $MC --room-join "$ROOM_ID" 2>&1; then
      pass "bob join"
    else
      fail "bob join"
    fi

    # ---- Phase 3: Send messages (all format types) ----
    echo ""
    echo "=== Bob sends messages ==="
    # Bob sends messages so alice's listen tail captures cross-user events
    if $MC --room "$ROOM_ID" -m "hello from bob" 2>&1; then
      pass "bob send plain text"
    else
      fail "bob send plain text"
    fi
    if $MC --room "$ROOM_ID" -m "bob notice" --notice 2>&1; then
      pass "bob send notice"
    else
      fail "bob send notice"
    fi
    # Python mc --emote returns non-zero (known bug) — send anyway for listen comparison
    if $MC --room "$ROOM_ID" -m "bob emote" --emote 2>&1; then
      pass "bob send emote"
    else
      skip "bob send emote (Python --emote exits non-zero)"
    fi

    cd "$HOME/alice-deviceA"

    echo ""
    echo "=== Alice sends messages ==="

    if $MC --room "$ROOM_ID" -m "plain text message" 2>&1; then
      pass "send plain text"
    else
      fail "send plain text"
    fi

    if $MC --room "$ROOM_ID" -m "**bold** and _italic_" --markdown 2>&1; then
      pass "send markdown"
    else
      fail "send markdown"
    fi

    if $MC --room "$ROOM_ID" -m "<b>html bold</b>" --html 2>&1; then
      pass "send html"
    else
      fail "send html"
    fi

    if $MC --room "$ROOM_ID" -m "code block test" --code 2>&1; then
      pass "send code"
    else
      fail "send code"
    fi

    if $MC --room "$ROOM_ID" -m "notice message" --notice 2>&1; then
      pass "send notice"
    else
      fail "send notice"
    fi

    # Python mc --emote returns non-zero (known bug) — send anyway for listen comparison
    if $MC --room "$ROOM_ID" -m "emote message" --emote 2>&1; then
      pass "send emote"
    else
      skip "send emote (Python --emote exits non-zero)"
    fi

    PEID_OUT=$($MC --room "$ROOM_ID" -m "print-event-id-test" --print-event-id 2>/dev/null) || true
    echo "$PEID_OUT" > "$HOME/outputs/print-event-id.out"
    if [ -n "$PEID_OUT" ]; then
      pass "send with print-event-id"
    else
      fail "send with print-event-id"
    fi

    # Create a test file for media upload
    echo "test file content for upload" > "$HOME/test-upload.txt"

    # ---- Phase 4: Capture ALL outputs (text + json) ----
    echo ""
    echo "=== Capturing outputs ==="

    # -- whoami --
    capture whoami text --whoami
    capture whoami json --whoami

    # -- devices --
    capture devices text --devices
    capture devices json --devices

    # -- joined-rooms --
    capture joined-rooms text --joined-rooms
    capture joined-rooms json --joined-rooms

    # -- joined-members --
    capture joined-members text --joined-members "$ROOM_ID"
    capture joined-members json --joined-members "$ROOM_ID"

    # -- get-room-info --
    capture get-room-info text --get-room-info "$ROOM_ID"
    capture get-room-info json --get-room-info "$ROOM_ID"

    # -- get-display-name --
    capture get-display-name text --get-display-name
    capture get-display-name json --get-display-name

    # -- get-profile --
    capture get-profile text --get-profile
    capture get-profile json --get-profile

    # -- login-info --
    capture login-info text --login-info
    capture login-info json --login-info

    # -- discovery-info --
    capture discovery-info text --discovery-info
    capture discovery-info json --discovery-info

    # -- content-repository-config --
    capture content-repository-config text --content-repository-config
    capture content-repository-config json --content-repository-config

    # -- room-get-visibility --
    capture room-get-visibility text --room-get-visibility "$ROOM_ID"
    capture room-get-visibility json --room-get-visibility "$ROOM_ID"

    # -- room-resolve-alias --
    capture room-resolve-alias text --room-resolve-alias "#test-room-py:${serverName}"
    capture room-resolve-alias json --room-resolve-alias "#test-room-py:${serverName}"

    # -- room-create (JSON only, creates a second room) --
    capture room-create json --room-create test-json-room-py --plain

    # -- media-upload --
    capture media-upload text --media-upload "$HOME/test-upload.txt"
    capture media-upload json --media-upload "$HOME/test-upload.txt"

    # -- listen tail (text + json) --
    timeout 30 $MC --listen tail --tail 10 --room "$ROOM_ID" \
      2>/dev/null > "$HOME/outputs/listen-text.out" || true
    timeout 30 $MC --listen tail --tail 10 --room "$ROOM_ID" --output json \
      2>/dev/null > "$HOME/outputs/listen-json.out" || true

    cd "$HOME"

    echo ""
    echo "============================================"
    echo "  Python Results: Failures=$failures"
    echo "============================================"
    exit $failures
  '';

  # ============================================================
  # Rust matrix-commander-ng test script
  # ============================================================
  mcRsTestScript = pkgs.writeShellScript "test-matrix-commander-ng" ''
    set -euo pipefail
    export HOME=/tmp/mc-rs-test
    mkdir -p "$HOME/outputs"
    cd "$HOME"

    MC="matrix-commander-ng"
    SERVER="http://localhost:${toString synapsePort}"

    failures=0
    fail() { echo "FAIL: $1"; failures=$((failures + 1)); }
    pass() { echo "PASS: $1"; }
    skip() { echo "SKIP: $1"; }

    # Save command output to a file (with --credentials/--store).
    # Usage: capture <name> <format> <mc-args...>
    capture() {
      local name="$1" fmt="$2"; shift 2
      local outfile="$HOME/outputs/$name-$fmt.out"
      if [ "$fmt" = "json" ]; then
        $MC --credentials ./credentials.json --store ./store/ "$@" --output json 2>/dev/null > "$outfile" || true
      else
        $MC --credentials ./credentials.json --store ./store/ "$@" 2>/dev/null > "$outfile" || true
      fi
    }

    echo "============================================"
    echo "  Testing matrix-commander-ng"
    echo "============================================"

    # ---- Phase 1: Login ----
    echo ""
    echo "=== Login alice device A ==="
    mkdir -p alice-deviceA && cd alice-deviceA
    $MC --login password \
      --homeserver "$SERVER" \
      --user-login "@${user1}:${serverName}" \
      --password "${user1Pass}" \
      --device "alice-deviceA-rs" \
      --room-default '!placeholder:${serverName}' \
      --credentials ./credentials.json --store ./store/ 2>&1 || true
    [ -f credentials.json ] && pass "alice deviceA login" || fail "alice deviceA login"
    cd "$HOME"

    echo ""
    echo "=== Login alice device B ==="
    mkdir -p alice-deviceB && cd alice-deviceB
    $MC --login password \
      --homeserver "$SERVER" \
      --user-login "@${user1}:${serverName}" \
      --password "${user1Pass}" \
      --device "alice-deviceB-rs" \
      --room-default '!placeholder:${serverName}' \
      --credentials ./credentials.json --store ./store/ 2>&1 || true
    [ -f credentials.json ] && pass "alice deviceB login" || fail "alice deviceB login"
    cd "$HOME"

    echo ""
    echo "=== Login bob ==="
    mkdir -p bob && cd bob
    $MC --login password \
      --homeserver "$SERVER" \
      --user-login "@${user2}:${serverName}" \
      --password "${user2Pass}" \
      --device "bob-device-rs" \
      --room-default '!placeholder:${serverName}' \
      --credentials ./credentials.json --store ./store/ 2>&1 || true
    [ -f credentials.json ] && pass "bob login" || fail "bob login"
    cd "$HOME"

    # ---- Phase 2: Setup ----
    cd alice-deviceA

    echo ""
    echo "=== Set display name ==="
    if $MC --credentials ./credentials.json --store ./store/ \
      --set-display-name "Alice Test" 2>&1; then
      pass "set display name"
    else
      fail "set display name"
    fi

    echo ""
    echo "=== Create room ==="
    ROOM_OUT=$($MC --credentials ./credentials.json --store ./store/ \
      --room-create test-room-rs --plain 2>&1) || true
    echo "RS_ROOM_CREATE=$ROOM_OUT"
    ROOM_ID=$(echo "$ROOM_OUT" | grep -oP '^!\S+' || echo "")
    if [ -n "$ROOM_ID" ]; then
      pass "create room - $ROOM_ID"
      python3 -c "
import json
c = json.load(open('credentials.json'))
c['room_id'] = '$ROOM_ID'
json.dump(c, open('credentials.json', 'w'))
"
    else
      fail "create room"
    fi

    echo ""
    echo "=== Invite bob and join ==="
    if $MC --credentials ./credentials.json --store ./store/ \
      --room-invite "$ROOM_ID" --user "@${user2}:${serverName}" 2>&1; then
      pass "invite bob"
    else
      fail "invite bob"
    fi
    cd "$HOME/bob"
    if $MC --credentials ./credentials.json --store ./store/ \
      --room-join "$ROOM_ID" 2>&1; then
      pass "bob join"
    else
      fail "bob join"
    fi

    # ---- Phase 3: Send messages (all format types) ----
    echo ""
    echo "=== Bob sends messages ==="
    # Bob sends messages so alice's listen tail captures cross-user events
    if $MC --credentials ./credentials.json --store ./store/ \
      --room "$ROOM_ID" -m "hello from bob" 2>&1; then
      pass "bob send plain text"
    else
      fail "bob send plain text"
    fi
    if $MC --credentials ./credentials.json --store ./store/ \
      --room "$ROOM_ID" -m "bob notice" --notice 2>&1; then
      pass "bob send notice"
    else
      fail "bob send notice"
    fi
    if $MC --credentials ./credentials.json --store ./store/ \
      --room "$ROOM_ID" -m "bob emote" --emote 2>&1; then
      pass "bob send emote"
    else
      fail "bob send emote"
    fi

    cd "$HOME/alice-deviceA"

    echo ""
    echo "=== Alice sends messages ==="

    if $MC --credentials ./credentials.json --store ./store/ \
      --room "$ROOM_ID" -m "plain text message" 2>&1; then
      pass "send plain text"
    else
      fail "send plain text"
    fi

    if $MC --credentials ./credentials.json --store ./store/ \
      --room "$ROOM_ID" -m "**bold** and _italic_" --markdown 2>&1; then
      pass "send markdown"
    else
      fail "send markdown"
    fi

    if $MC --credentials ./credentials.json --store ./store/ \
      --room "$ROOM_ID" -m "<b>html bold</b>" --html 2>&1; then
      pass "send html"
    else
      fail "send html"
    fi

    if $MC --credentials ./credentials.json --store ./store/ \
      --room "$ROOM_ID" -m "code block test" --code 2>&1; then
      pass "send code"
    else
      fail "send code"
    fi

    if $MC --credentials ./credentials.json --store ./store/ \
      --room "$ROOM_ID" -m "notice message" --notice 2>&1; then
      pass "send notice"
    else
      fail "send notice"
    fi

    if $MC --credentials ./credentials.json --store ./store/ \
      --room "$ROOM_ID" -m "emote message" --emote 2>&1; then
      pass "send emote"
    else
      fail "send emote"
    fi

    PEID_OUT=$($MC --credentials ./credentials.json --store ./store/ \
      --room "$ROOM_ID" -m "print-event-id-test" --print-event-id 2>/dev/null) || true
    echo "$PEID_OUT" > "$HOME/outputs/print-event-id.out"
    if [ -n "$PEID_OUT" ]; then
      pass "send with print-event-id"
    else
      fail "send with print-event-id"
    fi

    # Create a test file for media upload
    echo "test file content for upload" > "$HOME/test-upload.txt"

    # ---- Phase 4: Capture ALL outputs (text + json) ----
    echo ""
    echo "=== Capturing outputs ==="

    capture whoami text --whoami
    capture whoami json --whoami

    capture devices text --devices
    capture devices json --devices

    capture joined-rooms text --joined-rooms
    capture joined-rooms json --joined-rooms

    capture joined-members text --joined-members "$ROOM_ID"
    capture joined-members json --joined-members "$ROOM_ID"

    capture get-room-info text --get-room-info "$ROOM_ID"
    capture get-room-info json --get-room-info "$ROOM_ID"

    capture get-display-name text --get-display-name
    capture get-display-name json --get-display-name

    capture get-profile text --get-profile
    capture get-profile json --get-profile

    capture login-info text --login-info
    capture login-info json --login-info

    capture discovery-info text --discovery-info
    capture discovery-info json --discovery-info

    capture content-repository-config text --content-repository-config
    capture content-repository-config json --content-repository-config

    capture room-get-visibility text --room-get-visibility "$ROOM_ID"
    capture room-get-visibility json --room-get-visibility "$ROOM_ID"

    capture room-resolve-alias text --room-resolve-alias "#test-room-rs:${serverName}"
    capture room-resolve-alias json --room-resolve-alias "#test-room-rs:${serverName}"

    capture room-create json --room-create test-json-room-rs --plain

    capture media-upload text --media-upload "$HOME/test-upload.txt"
    capture media-upload json --media-upload "$HOME/test-upload.txt"

    # listen tail (text + json)
    timeout 30 $MC --listen tail --tail 10 --room "$ROOM_ID" \
      --credentials ./credentials.json --store ./store/ \
      2>/dev/null > "$HOME/outputs/listen-text.out" || true
    timeout 30 $MC --listen tail --tail 10 --room "$ROOM_ID" --output json \
      --credentials ./credentials.json --store ./store/ \
      2>/dev/null > "$HOME/outputs/listen-json.out" || true

    cd "$HOME"

    echo ""
    echo "============================================"
    echo "  Rust Results: Failures=$failures"
    echo "============================================"
    exit $failures
  '';

in
pkgs.testers.nixosTest {
  name = "matrix-commander-equalization";

  nodes.server = { config, pkgs, ... }: {
    services.matrix-synapse = {
      enable = true;
      settings = {
        server_name = serverName;
        public_baseurl = "http://localhost:${toString synapsePort}";

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
      };
    };

    environment.systemPackages = [
      pkgs.matrix-commander
      matrix-commander-ng-local
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

    server.start()
    server.wait_for_unit("matrix-synapse.service")
    server.wait_for_open_port(${toString synapsePort})

    # Verify Synapse is responding
    server.succeed("curl -sf http://localhost:${toString synapsePort}/_matrix/client/versions | jq .")

    # Register test users
    server.succeed("${registerUser} ${user1} ${user1Pass}")
    server.succeed("${registerUser} ${user2} ${user2Pass}")

    # Verify users can authenticate
    server.succeed(
        'curl -sf -X POST http://localhost:${toString synapsePort}/_matrix/client/v3/login '
        '-H "Content-Type: application/json" '
        '-d \'{"type": "m.login.password", "user": "${user1}", "password": "${user1Pass}"}\' '
        '| jq .access_token'
    )

    # Run both test suites
    with subtest("matrix-commander Python tests"):
        result = server.execute("${mcPyTestScript} 2>&1")
        print(f"=== matrix-commander (Python) exit code: {result[0]} ===")
        print(result[1])
        if result[0] != 0:
            raise Exception(f"matrix-commander Python tests failed with exit code {result[0]}")

    with subtest("matrix-commander-ng tests"):
        result = server.execute("${mcRsTestScript} 2>&1")
        print(f"=== matrix-commander-ng exit code: {result[0]} ===")
        print(result[1])
        if result[0] != 0:
            raise Exception(f"matrix-commander-ng tests failed with exit code {result[0]}")

    # ================================================================
    # COMPREHENSIVE OUTPUT PARITY COMPARISON
    # ================================================================
    with subtest("comprehensive output parity"):
        py_dir = "/tmp/mc-py-test/outputs"
        rs_dir = "/tmp/mc-rs-test/outputs"

        counts = {"pass": 0, "fail": 0, "skip": 0}
        parity_checks = []
        state = {"section": "", "py_sample": "", "rs_sample": ""}

        def read_output(path):
            """Read a file from the VM, return stripped content or empty string."""
            return server.succeed(f"cat {path} 2>/dev/null || true").strip()

        def parse_jsonl(raw):
            """Parse newline-delimited JSON into a list of objects."""
            results = []
            for line in raw.strip().split('\n'):
                line = line.strip()
                if line:
                    try:
                        results.append(json.loads(line))
                    except json.JSONDecodeError:
                        pass
            return results

        def _record(status, label, detail=""):
            counts[status] += 1
            prefix = {"pass": "OK", "fail": "FAIL", "skip": "SKIP"}[status]
            msg = f"  PARITY {prefix}: {label}"
            if detail:
                msg += f" -- {detail}"
            print(msg)
            parity_checks.append({
                "section": state["section"],
                "label": label,
                "status": status,
                "detail": detail,
                "py_sample": state["py_sample"][:500],
                "rs_sample": state["rs_sample"][:500],
            })

        def parity_ok(label, detail=""): _record("pass", label, detail)
        def parity_bad(label, detail=""): _record("fail", label, detail)
        def parity_na(label, detail=""): _record("skip", label, detail)

        def compare_json_keys(label, py_raw, rs_raw, is_jsonl=False):
            """Compare JSON key structure between Python and Rust outputs.
            Checks: key sets match, value types match."""
            if not py_raw and not rs_raw:
                parity_na(label, "both outputs empty")
                return

            if not py_raw:
                parity_na(label, "Python output empty")
                return

            if not rs_raw:
                parity_bad(label, "Rust output empty but Python produced output")
                return

            try:
                if is_jsonl:
                    py_items = parse_jsonl(py_raw)
                    rs_items = parse_jsonl(rs_raw)
                    if not py_items or not rs_items:
                        parity_na(label, f"JSONL: py={len(py_items)} rs={len(rs_items)} items")
                        return
                    py_j = py_items[0]
                    rs_j = rs_items[0]
                else:
                    # Take first line in case of multi-line output
                    py_j = json.loads(py_raw.split('\n')[0])
                    rs_j = json.loads(rs_raw.split('\n')[0])
            except json.JSONDecodeError as e:
                parity_bad(label, f"JSON parse error: {e}")
                print(f"    Python raw (first 200): {py_raw[:200]}")
                print(f"    Rust raw (first 200):   {rs_raw[:200]}")
                return

            if isinstance(py_j, dict) and isinstance(rs_j, dict):
                py_keys = set(py_j.keys())
                rs_keys = set(rs_j.keys())
                common = py_keys & rs_keys
                py_only = py_keys - rs_keys
                rs_only = rs_keys - py_keys

                if py_only or rs_only:
                    parity_bad(label, "key mismatch")
                    print(f"    Common keys:  {sorted(common)}")
                    if py_only:
                        print(f"    Python-only:  {sorted(py_only)}")
                    if rs_only:
                        print(f"    Rust-only:    {sorted(rs_only)}")
                    return

                # Keys match — now check value types
                type_mismatches = []
                for k in sorted(common):
                    py_t = type(py_j[k]).__name__
                    rs_t = type(rs_j[k]).__name__
                    # null vs string is a common mismatch to flag
                    if py_t != rs_t:
                        type_mismatches.append(f"{k}: py={py_t} rs={rs_t}")

                if type_mismatches:
                    parity_bad(label, f"type mismatches: {type_mismatches}")
                else:
                    parity_ok(label, f"keys={sorted(common)}")

            elif isinstance(py_j, list) and isinstance(rs_j, list):
                # Compare first element's structure if both are arrays of objects
                if py_j and rs_j and isinstance(py_j[0], dict) and isinstance(rs_j[0], dict):
                    py_keys = set(py_j[0].keys())
                    rs_keys = set(rs_j[0].keys())
                    py_only = py_keys - rs_keys
                    rs_only = rs_keys - py_keys
                    if py_only or rs_only:
                        parity_bad(label, "array element key mismatch")
                        if py_only:
                            print(f"    Python-only:  {sorted(py_only)}")
                        if rs_only:
                            print(f"    Rust-only:    {sorted(rs_only)}")
                    else:
                        parity_ok(label, f"array element keys={sorted(py_keys)}")
                else:
                    parity_ok(label, f"both arrays (py={len(py_j)} rs={len(rs_j)} items)")

            elif type(py_j).__name__ == type(rs_j).__name__:
                parity_ok(label, f"same type: {type(py_j).__name__}")
            else:
                parity_bad(label, f"type mismatch: py={type(py_j).__name__} rs={type(rs_j).__name__}")

        # ========================================
        # 1. Credential format comparison
        # ========================================
        state["section"] = "Credential Format"
        print("")
        print("=" * 60)
        print("  1. CREDENTIAL FORMAT")
        print("=" * 60)

        py_creds_raw = read_output("/tmp/mc-py-test/alice-deviceA/credentials.json")
        rs_creds_raw = read_output("/tmp/mc-rs-test/alice-deviceA/credentials.json")
        state["py_sample"] = py_creds_raw
        state["rs_sample"] = rs_creds_raw

        print(f"  Python: {py_creds_raw[:200]}")
        print(f"  Rust:   {rs_creds_raw[:200]}")

        compare_json_keys("credentials JSON keys", py_creds_raw, rs_creds_raw)

        try:
            py_creds = json.loads(py_creds_raw)
            rs_creds = json.loads(rs_creds_raw)

            # homeserver format (no trailing slash)
            py_hs = py_creds.get("homeserver", "")
            rs_hs = rs_creds.get("homeserver", "")
            if py_hs == rs_hs:
                parity_ok("credentials homeserver value", f"'{rs_hs}'")
            else:
                parity_bad("credentials homeserver value", f"py='{py_hs}' rs='{rs_hs}'")

            # refresh_token presence
            py_has_rt = "refresh_token" in py_creds
            rs_has_rt = "refresh_token" in rs_creds
            if py_has_rt == rs_has_rt:
                parity_ok("credentials refresh_token presence", f"both={'present' if py_has_rt else 'absent'}")
            else:
                parity_bad("credentials refresh_token presence", f"py={py_has_rt} rs={rs_has_rt}")
        except json.JSONDecodeError:
            parity_bad("credentials parse", "could not parse one or both")

        # ========================================
        # 2. JSON output comparison for all commands
        # ========================================
        state["section"] = "JSON Output"
        print("")
        print("=" * 60)
        print("  2. JSON OUTPUT PARITY (per-command)")
        print("=" * 60)

        # Commands and whether they produce JSONL (one object per line)
        json_commands = [
            ("whoami",                    False),
            ("devices",                   True),   # one JSON object per device
            ("joined-rooms",              False),
            ("joined-members",            False),
            ("get-room-info",             False),
            ("get-display-name",          False),
            ("get-profile",               False),
            ("login-info",                False),
            ("discovery-info",            False),
            ("content-repository-config", False),
            ("room-get-visibility",       False),
            ("room-resolve-alias",        False),
            ("room-create",               False),
            ("media-upload",              False),
        ]

        for cmd, is_jsonl in json_commands:
            print(f"\n  --- --{cmd} --output json ---")
            py_json = read_output(f"{py_dir}/{cmd}-json.out")
            rs_json = read_output(f"{rs_dir}/{cmd}-json.out")
            state["py_sample"] = py_json
            state["rs_sample"] = rs_json

            # Print raw samples for debugging
            if py_json:
                print(f"    Python: {py_json[:200]}")
            if rs_json:
                print(f"    Rust:   {rs_json[:200]}")

            compare_json_keys(f"--{cmd} JSON", py_json, rs_json, is_jsonl=is_jsonl)

        # ========================================
        # 3. Text output format comparison
        # ========================================
        state["section"] = "Text Output"
        print("")
        print("=" * 60)
        print("  3. TEXT OUTPUT PARITY (format checks)")
        print("=" * 60)

        # -- whoami --
        print("\n  --- --whoami text ---")
        py_whoami = read_output(f"{py_dir}/whoami-text.out")
        rs_whoami = read_output(f"{rs_dir}/whoami-text.out")
        state["py_sample"] = py_whoami
        state["rs_sample"] = rs_whoami
        print(f"    Python: {py_whoami}")
        print(f"    Rust:   {rs_whoami}")
        if py_whoami and rs_whoami:
            if "@${user1}" in py_whoami and "@${user1}" in rs_whoami:
                parity_ok("--whoami text", "both contain @${user1}")
            else:
                parity_bad("--whoami text", f"py='{py_whoami}' rs='{rs_whoami}'")
        else:
            parity_na("--whoami text", "missing output")

        # -- joined-rooms: should be room IDs only (no Room: prefix) --
        print("\n  --- --joined-rooms text ---")
        py_jr = read_output(f"{py_dir}/joined-rooms-text.out")
        rs_jr = read_output(f"{rs_dir}/joined-rooms-text.out")
        state["py_sample"] = py_jr
        state["rs_sample"] = rs_jr
        print(f"    Python: {py_jr[:200]}")
        print(f"    Rust:   {rs_jr[:200]}")
        if rs_jr:
            rs_lines = [l for l in rs_jr.split('\n') if l.strip()]
            if all(l.strip().startswith('!') for l in rs_lines) and "Room:" not in rs_jr:
                parity_ok("--joined-rooms text", "room IDs only, no 'Room:' prefix")
            else:
                parity_bad("--joined-rooms text", f"unexpected format: {rs_jr[:100]}")
        else:
            parity_na("--joined-rooms text", "Rust output empty")

        # -- devices: should not have Device: prefix --
        print("\n  --- --devices text ---")
        py_dev = read_output(f"{py_dir}/devices-text.out")
        rs_dev = read_output(f"{rs_dir}/devices-text.out")
        state["py_sample"] = py_dev
        state["rs_sample"] = rs_dev
        print(f"    Python: {py_dev[:200]}")
        print(f"    Rust:   {rs_dev[:200]}")
        if rs_dev:
            if "Device:" not in rs_dev:
                parity_ok("--devices text", "tabular format (no 'Device:' prefix)")
            else:
                parity_bad("--devices text", "has 'Device:' prefix")
        else:
            parity_na("--devices text", "Rust output empty")

        # -- print-event-id: $event_id    !room_id    message --
        print("\n  --- --print-event-id ---")
        py_peid = read_output(f"{py_dir}/print-event-id.out")
        rs_peid = read_output(f"{rs_dir}/print-event-id.out")
        state["py_sample"] = py_peid
        state["rs_sample"] = rs_peid
        print(f"    Python: {py_peid[:200]}")
        print(f"    Rust:   {rs_peid[:200]}")
        if py_peid and rs_peid:
            py_ok = py_peid.startswith("$") and "    " in py_peid
            rs_ok = rs_peid.startswith("$") and "    " in rs_peid
            if py_ok and rs_ok:
                py_fields = len(py_peid.strip().split("    "))
                rs_fields = len(rs_peid.strip().split("    "))
                if py_fields == rs_fields:
                    parity_ok("--print-event-id", f"format matches ({py_fields} fields)")
                else:
                    parity_bad("--print-event-id", f"field count: py={py_fields} rs={rs_fields}")
            else:
                parity_bad("--print-event-id", f"format: py_ok={py_ok} rs_ok={rs_ok}")
        else:
            parity_na("--print-event-id", "missing output")

        # -- login-info: both should show m.login.password --
        print("\n  --- --login-info text ---")
        py_li = read_output(f"{py_dir}/login-info-text.out")
        rs_li = read_output(f"{rs_dir}/login-info-text.out")
        state["py_sample"] = py_li
        state["rs_sample"] = rs_li
        print(f"    Python: {py_li[:200]}")
        print(f"    Rust:   {rs_li[:200]}")
        if py_li and rs_li:
            if "m.login.password" in py_li and "m.login.password" in rs_li:
                parity_ok("--login-info text", "both show m.login.password")
            else:
                parity_bad("--login-info text", "missing m.login.password")
        else:
            parity_na("--login-info text", "missing output")

        # -- content-repository-config: should be numeric --
        print("\n  --- --content-repository-config text ---")
        py_crc = read_output(f"{py_dir}/content-repository-config-text.out")
        rs_crc = read_output(f"{rs_dir}/content-repository-config-text.out")
        state["py_sample"] = py_crc
        state["rs_sample"] = rs_crc
        print(f"    Python: {py_crc}")
        print(f"    Rust:   {rs_crc}")
        if rs_crc:
            try:
                int(rs_crc.strip())
                parity_ok("--content-repository-config text", f"numeric: {rs_crc.strip()}")
            except ValueError:
                parity_bad("--content-repository-config text", f"not numeric: {rs_crc}")
        else:
            parity_na("--content-repository-config text", "Rust output empty")

        # -- discovery-info: should contain homeserver URL --
        print("\n  --- --discovery-info text ---")
        py_di = read_output(f"{py_dir}/discovery-info-text.out")
        rs_di = read_output(f"{rs_dir}/discovery-info-text.out")
        state["py_sample"] = py_di
        state["rs_sample"] = rs_di
        print(f"    Python: {py_di[:200]}")
        print(f"    Rust:   {rs_di[:200]}")
        if py_di and rs_di:
            if "localhost" in py_di and "localhost" in rs_di:
                parity_ok("--discovery-info text", "both contain 'localhost'")
            else:
                parity_bad("--discovery-info text", "missing 'localhost'")
        else:
            parity_na("--discovery-info text", "missing output")

        # -- get-display-name: should contain "Alice Test" --
        print("\n  --- --get-display-name text ---")
        py_dn = read_output(f"{py_dir}/get-display-name-text.out")
        rs_dn = read_output(f"{rs_dir}/get-display-name-text.out")
        state["py_sample"] = py_dn
        state["rs_sample"] = rs_dn
        print(f"    Python: {py_dn}")
        print(f"    Rust:   {rs_dn}")
        if py_dn and rs_dn:
            if "Alice Test" in py_dn and "Alice Test" in rs_dn:
                parity_ok("--get-display-name text", "both contain 'Alice Test'")
            else:
                parity_bad("--get-display-name text", f"py='{py_dn}' rs='{rs_dn}'")
        else:
            parity_na("--get-display-name text", "missing output")

        # -- room-get-visibility: should contain public or private --
        print("\n  --- --room-get-visibility text ---")
        py_vis = read_output(f"{py_dir}/room-get-visibility-text.out")
        rs_vis = read_output(f"{rs_dir}/room-get-visibility-text.out")
        state["py_sample"] = py_vis
        state["rs_sample"] = rs_vis
        print(f"    Python: {py_vis}")
        print(f"    Rust:   {rs_vis}")
        if py_vis and rs_vis:
            py_is_private = "private" in py_vis.lower()
            rs_is_private = "private" in rs_vis.lower()
            if py_is_private == rs_is_private:
                parity_ok("--room-get-visibility text", "both agree on visibility")
            else:
                parity_bad("--room-get-visibility text", f"py='{py_vis}' rs='{rs_vis}'")
        else:
            parity_na("--room-get-visibility text", "missing output")

        # ========================================
        # 4. Listen output comparison
        # ========================================
        state["section"] = "Listen Output"
        print("")
        print("=" * 60)
        print("  4. LISTEN OUTPUT PARITY")
        print("=" * 60)

        # -- Listen JSON --
        print("\n  --- --listen tail --output json ---")
        py_listen_json = read_output(f"{py_dir}/listen-json.out")
        rs_listen_json = read_output(f"{rs_dir}/listen-json.out")
        state["py_sample"] = py_listen_json
        state["rs_sample"] = rs_listen_json

        py_events = parse_jsonl(py_listen_json)
        rs_events = parse_jsonl(rs_listen_json)

        print(f"    Python events: {len(py_events)}")
        print(f"    Rust events:   {len(rs_events)}")

        if py_events and rs_events:
            # Compare event structure: find m.text events
            def get_events_by_msgtype(events, msgtype):
                result = []
                for e in events:
                    if not isinstance(e, dict):
                        continue
                    # Events may have content.msgtype, top-level msgtype,
                    # or source.content.msgtype (Python nio wrapper)
                    mt = None
                    if "msgtype" in e:
                        mt = e["msgtype"]
                    elif "content" in e and isinstance(e["content"], dict):
                        mt = e["content"].get("msgtype")
                    elif "source" in e and isinstance(e["source"], dict):
                        src = e["source"]
                        if "content" in src and isinstance(src["content"], dict):
                            mt = src["content"].get("msgtype")
                    if mt == msgtype:
                        result.append(e)
                return result

            py_text = get_events_by_msgtype(py_events, "m.text")
            rs_text = get_events_by_msgtype(rs_events, "m.text")

            if py_text and rs_text:
                # Python wraps events: {source: <raw_event>, room, sender_nick, ...}
                # Rust outputs raw events: {content, sender, event_id, ...}
                # Compare the inner source keys (Python) vs raw keys (Rust)
                py_evt = py_text[0]
                rs_evt = rs_text[0]
                py_inner = py_evt.get("source", py_evt) if isinstance(py_evt, dict) else py_evt
                py_keys = set(py_inner.keys()) if isinstance(py_inner, dict) else set()
                rs_keys = set(rs_evt.keys())

                print(f"    m.text Python source keys: {sorted(py_keys)}")
                print(f"    m.text Rust keys:          {sorted(rs_keys)}")

                # Check required keys are present in both
                required = {"event_id", "sender", "origin_server_ts", "room_id"}

                rs_all_keys = set(rs_evt.keys())
                if "content" in rs_evt and isinstance(rs_evt["content"], dict):
                    rs_all_keys |= set(rs_evt["content"].keys())

                missing_required = required - rs_all_keys
                if missing_required:
                    parity_bad("listen JSON m.text required keys", f"missing: {sorted(missing_required)}")
                else:
                    parity_ok("listen JSON m.text required keys", f"all present: {sorted(required)}")

                # Check body field
                rs_has_body = "body" in rs_all_keys
                if rs_has_body:
                    parity_ok("listen JSON m.text has 'body'")
                else:
                    parity_bad("listen JSON m.text has 'body'", "missing 'body' field")

                # Compare source-level key sets
                common = py_keys & rs_keys
                py_only = py_keys - rs_keys
                rs_only = rs_keys - py_keys
                if py_only or rs_only:
                    print(f"    INFO: key differences: py_only={sorted(py_only)} rs_only={sorted(rs_only)}")
                    print(f"    INFO: common keys: {sorted(common)}")
                    if py_only:
                        parity_bad("listen JSON m.text key parity", f"Rust missing: {sorted(py_only)}")
                    else:
                        parity_ok("listen JSON m.text key parity", "Rust has all Python source keys (plus extras)")
                else:
                    parity_ok("listen JSON m.text key parity", f"exact match: {sorted(common)}")
            else:
                parity_na("listen JSON m.text", f"events: py={len(py_text)} rs={len(rs_text)}")

            # Check presence of notice events
            py_has_notice = len(get_events_by_msgtype(py_events, "m.notice")) > 0
            rs_has_notice = len(get_events_by_msgtype(rs_events, "m.notice")) > 0
            if py_has_notice and rs_has_notice:
                parity_ok("listen JSON notice", "both have events")
            elif not py_has_notice and not rs_has_notice:
                parity_na("listen JSON notice", "neither has events")
            else:
                parity_bad("listen JSON notice", f"py={py_has_notice} rs={rs_has_notice}")

            # Check that both output same number of events (approximately)
            diff = abs(len(py_events) - len(rs_events))
            if diff <= 2:
                parity_ok("listen JSON event count", f"py={len(py_events)} rs={len(rs_events)}")
            else:
                parity_bad("listen JSON event count", f"py={len(py_events)} rs={len(rs_events)}")
        elif not py_events and not rs_events:
            parity_na("listen JSON", "both empty")
        else:
            parity_bad("listen JSON", f"one side empty: py={len(py_events)} rs={len(rs_events)}")

        # -- Listen text --
        print("\n  --- --listen tail text ---")
        py_listen_text = read_output(f"{py_dir}/listen-text.out")
        rs_listen_text = read_output(f"{rs_dir}/listen-text.out")
        state["py_sample"] = py_listen_text
        state["rs_sample"] = rs_listen_text

        py_lines = [l for l in py_listen_text.split('\n') if l.strip()] if py_listen_text else []
        rs_lines = [l for l in rs_listen_text.split('\n') if l.strip()] if rs_listen_text else []

        print(f"    Python lines: {len(py_lines)}")
        print(f"    Rust lines:   {len(rs_lines)}")

        if py_lines and rs_lines:
            # Both should contain bob's messages (alice's are filtered by --listen-self)
            # Note: Python mc doesn't output emotes in listen tail, so only check text and notice
            for msg in ["hello from bob", "bob notice"]:
                py_has = any(msg in l for l in py_lines)
                rs_has = any(msg in l for l in rs_lines)
                if py_has and rs_has:
                    parity_ok(f"listen text contains '{msg}'")
                elif not py_has and not rs_has:
                    parity_na(f"listen text contains '{msg}'", "neither has it")
                else:
                    parity_bad(f"listen text contains '{msg}'", f"py={py_has} rs={rs_has}")

            # Check text format: "Message received for room ... | sender ... | datetime | body"
            if rs_lines:
                sample = rs_lines[0]
                has_format = "Message received for room" in sample and " | sender " in sample
                if has_format:
                    parity_ok("listen text format", "matches 'Message received for room ... | sender ...'")
                else:
                    parity_bad("listen text format", f"unexpected: {sample[:100]}")
        else:
            parity_na("listen text", f"insufficient output (py={len(py_lines)} rs={len(rs_lines)})")

        # ========================================
        # 5. File tree comparison (informational)
        # ========================================
        state["section"] = "File Trees"
        state["py_sample"] = ""
        state["rs_sample"] = ""
        print("")
        print("=" * 60)
        print("  5. FILE TREES (informational)")
        print("=" * 60)

        py_tree = server.succeed("find /tmp/mc-py-test -type f -not -path '*/store/*' -not -path '*/nio_store/*' 2>/dev/null | sort | head -30")
        rs_tree = server.succeed("find /tmp/mc-rs-test -type f -not -path '*/store/*' 2>/dev/null | sort | head -30")
        print(f"  Python files:\n{py_tree}")
        print(f"  Rust files:\n{rs_tree}")

        # ========================================
        # SUMMARY
        # ========================================
        print("")
        print("=" * 60)
        total = counts["pass"] + counts["fail"] + counts["skip"]
        print(f"  PARITY RESULTS: {counts['pass']} pass, {counts['fail']} fail, {counts['skip']} skip (of {total} checks)")
        print("=" * 60)

        # Write structured parity results to $out/
        out_dir = os.environ.get("out", "/tmp/eq-test-out")
        pathlib.Path(out_dir).mkdir(parents=True, exist_ok=True)
        parity_summary = {
            "total": total,
            "pass": counts["pass"],
            "fail": counts["fail"],
            "skip": counts["skip"],
            "checks": parity_checks,
        }
        parity_path = pathlib.Path(out_dir) / "parity-summary.json"
        with open(parity_path, "w") as f:
            json.dump(parity_summary, f, indent=2)
        print(f"  Wrote parity summary to {parity_path}")

        if counts["fail"] > 0:
            raise Exception(f"{counts['fail']} parity check(s) failed (results saved to {parity_path})")
  '';
}
