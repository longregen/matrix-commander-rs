{
  description = "NixOS integration tests for matrix-commander (Python) and matrix-commander-ng equivalence";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    mc-rs-src = {
      url = "path:..";
      flake = false;
    };
  };

  outputs = { self, nixpkgs, mc-rs-src }:
    let
      # All platforms that the Rust package can build on
      allSystems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];
      forAllSystems = fn: nixpkgs.lib.genAttrs allSystems fn;

      # NixOS VM tests and the dev sandbox require KVM (x86_64-linux only)
      linuxSystem = "x86_64-linux";

      mkPkgsFor = system: import nixpkgs {
        inherit system;
        config = {
          permittedInsecurePackages = [ "olm-3.2.16" ];
        };
      };

      # x86_64-linux pkgs used for checks, devShell, sandbox, etc.
      pkgs = mkPkgsFor linuxSystem;

      # Python environment with all matrix-commander dependencies
      pythonEnv = pkgs.python3.withPackages (ps: [
        ps.aiohttp
        ps.aiofiles
        ps.emoji
        ps.markdown
        (ps.matrix-nio.override { withOlm = true; })
        ps.pillow
        ps.python-magic
        ps.pyxdg
        ps.notify2
        ps.async-timeout

        # e2e encryption deps (pulled in by matrix-nio[e2e] but explicit for clarity)
        ps.python-olm
        ps.peewee
        ps.cachetools
        ps.atomicwrites

        # useful for testing
        ps.requests
        ps.pytest
        ps.playwright
      ]);

      # Rust build inputs for matrix-commander-ng
      rustBuildInputs = with pkgs; [
        openssl
        pkg-config
        sqlite
      ];

      # Build matrix-commander-ng for a given system
      mkMatrixCommanderNg = system:
        let sysPkgs = mkPkgsFor system;
        in sysPkgs.rustPlatform.buildRustPackage {
          pname = "matrix-commander-ng";
          version = "1.0.0-local";

          src = sysPkgs.lib.cleanSourceWith {
            src = mc-rs-src;
            filter = path: type:
              let baseName = baseNameOf path; in
              (type == "directory" && builtins.elem baseName [ "src" ".cargo" ]) ||
              (sysPkgs.lib.hasSuffix ".rs" baseName) ||
              (sysPkgs.lib.hasSuffix ".toml" baseName) ||
              (baseName == "Cargo.lock");
          };

          cargoLock.lockFile = "${mc-rs-src}/Cargo.lock";

          nativeBuildInputs = with sysPkgs; [ pkg-config perl ];
          buildInputs = with sysPkgs; [ openssl ]
            ++ sysPkgs.lib.optionals sysPkgs.stdenv.isDarwin
              (with sysPkgs.darwin.apple_sdk.frameworks; [ Security SystemConfiguration ]);

          meta = {
            description = "CLI-based Matrix client app for sending and receiving (local build)";
            mainProgram = "matrix-commander-ng";
          };
        };

      # Local build for x86_64-linux (used by checks, devShell, sandbox, etc.)
      matrix-commander-ng-local = mkMatrixCommanderNg linuxSystem;

      # Element Web configured for local Synapse
      elementWebConfigured = pkgs.element-web.override {
        conf = {
          default_server_config = {
            "m.homeserver" = {
              base_url = "http://localhost:8008";
              server_name = "localhost";
            };
          };
          disable_custom_urls = true;
          disable_guests = true;
          brand = "Element";
        };
      };

      # Cinny configured for local Synapse
      cinnyConfigured = pkgs.cinny.override {
        conf = {
          defaultHomeserver = 0;
          homeserverList = [ "http://localhost:8008" ];
          allowCustomHomeservers = true;
        };
      };

      # Nginx config for serving web clients
      nginxConfigTemplate = pkgs.writeText "nginx.conf" ''
        worker_processes 1;
        daemon on;
        pid __DATA_DIR__/nginx/nginx.pid;
        error_log __DATA_DIR__/nginx/error.log;
        events { worker_connections 64; }
        http {
          include ${pkgs.nginx}/conf/mime.types;
          default_type application/octet-stream;
          access_log __DATA_DIR__/nginx/access.log;
          client_body_temp_path __DATA_DIR__/nginx/tmp/client_body;
          proxy_temp_path __DATA_DIR__/nginx/tmp/proxy;
          fastcgi_temp_path __DATA_DIR__/nginx/tmp/fastcgi;
          uwsgi_temp_path __DATA_DIR__/nginx/tmp/uwsgi;
          scgi_temp_path __DATA_DIR__/nginx/tmp/scgi;

          server {
            listen 8090;
            server_name localhost;
            root ${elementWebConfigured};
            location / { try_files $uri $uri/ /index.html; }
          }
          server {
            listen 8091;
            server_name localhost;
            root ${cinnyConfigured};
            location / { try_files $uri $uri/ /index.html; }
          }
        }
      '';

      # All packages needed in the dev environment
      devPackages = [
        # -- Python (matrix-commander) --
        pythonEnv
        pkgs.file # libmagic runtime

        # -- Rust (matrix-commander-ng) --
        pkgs.cargo
        pkgs.rustc
        pkgs.clippy
        pkgs.rustfmt

        # -- Matrix homeserver --
        pkgs.matrix-synapse

        # -- Database --
        pkgs.postgresql

        # -- Sandbox / shell --
        pkgs.bubblewrap
        pkgs.fish

        # -- Utilities --
        pkgs.jq
        pkgs.curl
        pkgs.bash
        pkgs.coreutils
        pkgs.gnugrep
        pkgs.gnused
        pkgs.gawk
        pkgs.findutils
        pkgs.procps   # ps, kill, etc.
        pkgs.iproute2  # ip (for debugging network ns)
        pkgs.openssl   # for generating keys / certs

        # -- Web clients --
        elementWebConfigured
        cinnyConfigured
        pkgs.nginx

        # -- Browser testing --
        pkgs.playwright-driver.browsers

        # -- CLI tools (nixpkgs builds, for browser cross-device tests) --
        pkgs.matrix-commander
        pkgs.matrix-commander-rs
      ] ++ rustBuildInputs;

      # Synapse override config template (written at runtime with env vars substituted)
      synapseOverrideTemplate = pkgs.writeText "synapse-override-template.yaml" ''
        database:
          name: psycopg2
          args:
            user: synapse
            database: synapse
            host: __PGDATA__
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
          - port: __SYNAPSE_PORT__
            type: http
            tls: false
            bind_addresses: ['127.0.0.1']
            resources:
              - names: [client, federation]
                compress: false

        suppress_key_server_warning: true
      '';

      # Fish init script sourced inside the sandbox
      fishInitScript = pkgs.writeText "mc-sandbox-init.fish" ''
        set -gx fish_greeting ""

        function init-postgres -d "Initialize PostgreSQL data directory"
          if not test -d "$PGDATA/base"
            echo "Initializing PostgreSQL data directory..."
            initdb --no-locale --encoding=UTF8 -D "$PGDATA"
            printf "unix_socket_directories = '%s'\n" "$PGDATA" >> "$PGDATA/postgresql.conf"
            printf "listen_addresses = ${"''"}\n" >> "$PGDATA/postgresql.conf"
            printf "port = %s\n" "$PGPORT" >> "$PGDATA/postgresql.conf"
          end
        end

        function start-postgres -d "Start PostgreSQL"
          init-postgres
          if not pg_isready -q 2>/dev/null
            echo "Starting PostgreSQL..."
            pg_ctl -D "$PGDATA" -l "$DATA_DIR/postgres.log" start -w
            createuser --no-superuser --no-createdb --no-createrole synapse 2>/dev/null; or true
            createdb --owner=synapse synapse 2>/dev/null; or true
          else
            echo "PostgreSQL already running."
          end
        end

        function stop-postgres -d "Stop PostgreSQL"
          if pg_isready -q 2>/dev/null
            echo "Stopping PostgreSQL..."
            pg_ctl -D "$PGDATA" stop
          end
        end

        function init-synapse -d "Generate Synapse config"
          if not test -f "$SYNAPSE_CONFIG"
            echo "Generating Synapse config..."
            mkdir -p "$SYNAPSE_DATA"
            synapse_homeserver \
              --server-name "$SYNAPSE_SERVER_NAME" \
              --config-path "$SYNAPSE_CONFIG" \
              --data-directory "$SYNAPSE_DATA" \
              --generate-config \
              --report-stats=no

            sed -e "s|__PGDATA__|$PGDATA|g" \
                -e "s|__SYNAPSE_PORT__|$SYNAPSE_PORT|g" \
                ${synapseOverrideTemplate} > "$SYNAPSE_DATA/homeserver-override.yaml"
          end
        end

        function start-synapse -d "Start Synapse"
          start-postgres
          init-synapse
          echo "Starting Synapse on port $SYNAPSE_PORT..."
          synapse_homeserver \
            --config-path "$SYNAPSE_CONFIG" \
            --config-path "$SYNAPSE_DATA/homeserver-override.yaml" \
            -D
          sleep 1
          pgrep -f 'synapse.app.homeserver' > "$SYNAPSE_DATA/synapse.pid" 2>/dev/null; or true
          echo "Synapse running at http://127.0.0.1:$SYNAPSE_PORT"
        end

        function stop-synapse -d "Stop Synapse"
          if test -f "$SYNAPSE_DATA/synapse.pid"
            echo "Stopping Synapse..."
            kill (cat "$SYNAPSE_DATA/synapse.pid") 2>/dev/null; or true
            rm -f "$SYNAPSE_DATA/synapse.pid"
          end
        end

        function register-user -d "Register a test user" -a username password
          if test -z "$username"
            echo "usage: register-user <username> [password]"
            return 1
          end
          set -q password[1]; or set password "$username"
          register_new_matrix_user \
            -u "$username" \
            -p "$password" \
            -c "$SYNAPSE_CONFIG" \
            --no-admin \
            "http://127.0.0.1:$SYNAPSE_PORT"
        end

        function register-admin -d "Register an admin user" -a username password
          if test -z "$username"
            echo "usage: register-admin <username> [password]"
            return 1
          end
          set -q password[1]; or set password "$username"
          register_new_matrix_user \
            -u "$username" \
            -p "$password" \
            -c "$SYNAPSE_CONFIG" \
            --admin \
            "http://127.0.0.1:$SYNAPSE_PORT"
        end

        function start-nginx -d "Start nginx for Element Web + Cinny"
          mkdir -p "$DATA_DIR/nginx/tmp"
          sed "s|__DATA_DIR__|$DATA_DIR|g" "$NGINX_CONFIG_TEMPLATE" > "$DATA_DIR/nginx/nginx.conf"
          nginx -c "$DATA_DIR/nginx/nginx.conf"
          echo "Element Web: http://127.0.0.1:$ELEMENT_WEB_PORT"
          echo "Cinny: http://127.0.0.1:$CINNY_PORT"
        end

        function stop-nginx -d "Stop nginx"
          if test -f "$DATA_DIR/nginx/nginx.pid"
            kill (cat "$DATA_DIR/nginx/nginx.pid") 2>/dev/null; or true
          end
        end

        function start-all -d "Start Postgres + Synapse + Nginx"
          start-synapse
          start-nginx
          echo ""
          echo "=== Environment ready ==="
          echo "  Synapse:     http://127.0.0.1:$SYNAPSE_PORT"
          echo "  Postgres:    $PGDATA (unix socket)"
          echo "  Element Web: http://127.0.0.1:$ELEMENT_WEB_PORT"
          echo "  Cinny:       http://127.0.0.1:$CINNY_PORT"
          echo ""
          echo "  register-user <name> [password]   - create a test user"
          echo "  register-admin <name> [password]  - create an admin user"
          echo "  stop-all                          - shut everything down"
        end

        function stop-all -d "Stop Nginx + Synapse + Postgres"
          stop-nginx
          stop-synapse
          stop-postgres
        end

        function clean-all -d "Stop everything and delete dev data"
          stop-all
          echo "Removing all dev data..."
          rm -rf "$DATA_DIR"
        end

        function build-rs -d "Build matrix-commander-ng"
          echo "Building matrix-commander-ng..."
          cargo build --manifest-path "$PROJECT_ROOT/matrix-commander-rs/Cargo.toml" $argv
        end

        echo ""
        echo "=== matrix-commander sandbox (isolated) ==="
        echo "  Network:  isolated (localhost only)"
        echo "  PIDs:     isolated (all processes die on exit)"
        echo "  Data:     $DATA_DIR"
        echo ""
        echo "Commands:"
        echo "  start-all       - Start Postgres + Synapse + Nginx"
        echo "  stop-all        - Stop everything"
        echo "  clean-all       - Stop everything and delete dev data"
        echo "  start-nginx     - Start nginx (Element Web + Cinny)"
        echo "  stop-nginx      - Stop nginx"
        echo "  register-user   - Register a test user"
        echo "  register-admin  - Register an admin user"
        echo "  build-rs        - Build matrix-commander-ng"
        echo ""
      '';

      # Script that enters an isolated bubblewrap sandbox running fish
      # Usage: mc-sandbox [project-root]            — interactive fish shell
      #        mc-sandbox --run <command> [args...]  — run a command, then exit
      sandboxScript = pkgs.writeShellScriptBin "mc-sandbox" ''
        set -euo pipefail

        PROJECT_ROOT="''${MC_PROJECT_ROOT:-$(pwd)}"
        RUN_CMD=()

        while [ $# -gt 0 ]; do
          case "$1" in
            --run) shift; RUN_CMD=("$@"); break ;;
            *)
              if [ -d "$1" ]; then
                PROJECT_ROOT="$(cd "$1" && pwd)"
              fi
              shift ;;
          esac
        done

        DATA_DIR="$PROJECT_ROOT/.dev-data"
        mkdir -p "$DATA_DIR"

        SANDBOX_PATH="${pkgs.lib.makeBinPath devPackages}"

        BWRAP_ARGS=(
          --ro-bind /nix/store /nix/store
          --ro-bind /etc /etc
          --symlink ${pkgs.bash}/bin/bash /bin/sh
          --symlink ${pkgs.coreutils}/bin/env /usr/bin/env
          --bind "$PROJECT_ROOT" "$PROJECT_ROOT"
          --bind "$DATA_DIR" "$DATA_DIR"
          --tmpfs /tmp
          --tmpfs /run
          --dev /dev
          --tmpfs /dev/shm
          --proc /proc
          --unshare-net
          --unshare-pid
          --die-with-parent
          --setenv PROJECT_ROOT "$PROJECT_ROOT"
          --setenv DATA_DIR "$DATA_DIR"
          --setenv PATH "$SANDBOX_PATH"
          --setenv PGDATA "$DATA_DIR/postgres"
          --setenv PGHOST "$DATA_DIR/postgres"
          --setenv PGPORT "5432"
          --setenv PGDATABASE "synapse"
          --setenv SYNAPSE_DATA "$DATA_DIR/synapse"
          --setenv SYNAPSE_CONFIG "$DATA_DIR/synapse/homeserver.yaml"
          --setenv SYNAPSE_LOG_CONFIG "$DATA_DIR/synapse/log.config"
          --setenv SYNAPSE_SERVER_NAME "localhost"
          --setenv SYNAPSE_PORT "8008"
          --setenv CARGO_HOME "$DATA_DIR/cargo"
          --setenv OPENSSL_DIR "${pkgs.openssl.dev}"
          --setenv OPENSSL_LIB_DIR "${pkgs.lib.getLib pkgs.openssl}/lib"
          --setenv PKG_CONFIG_PATH "${pkgs.openssl.dev}/lib/pkgconfig:${pkgs.sqlite.dev}/lib/pkgconfig"
          --setenv MC_SANDBOXED "1"
          --setenv HOME "/tmp/home"
          --setenv PLAYWRIGHT_BROWSERS_PATH "${pkgs.playwright-driver.browsers}"
          --setenv PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD "1"
          --setenv ELEMENT_WEB_PORT "8090"
          --setenv CINNY_PORT "8091"
          --setenv ELEMENT_WEB_URL "http://localhost:8090"
          --setenv CINNY_URL "http://localhost:8091"
          --setenv NGINX_CONFIG_TEMPLATE "${nginxConfigTemplate}"
          --chdir "$PROJECT_ROOT"
        )

        if [ ''${#RUN_CMD[@]} -gt 0 ]; then
          exec ${pkgs.bubblewrap}/bin/bwrap "''${BWRAP_ARGS[@]}" -- ${pkgs.bash}/bin/bash -c 'exec "$@"' -- "''${RUN_CMD[@]}"
        else
          exec ${pkgs.bubblewrap}/bin/bwrap "''${BWRAP_ARGS[@]}" -- ${pkgs.fish}/bin/fish --init-command "source ${fishInitScript}"
        fi
      '';

    in
    {
      # Packages for all platforms
      packages = forAllSystems (system: {
        default = mkMatrixCommanderNg system;
        matrix-commander-ng = mkMatrixCommanderNg system;
      });

      # NixOS VM tests require KVM — x86_64-linux only
      checks.${linuxSystem} = {
        matrix-commander-test = import ./tests/nixos-test.nix {
          inherit pkgs;
          inherit (pkgs) lib;
          matrix-commander-ng-local = matrix-commander-ng-local;
        };
        integration-test = import "${mc-rs-src}/tests/nixos-test.nix" {
          inherit pkgs;
          inherit (pkgs) lib;
          matrix-commander-ng = matrix-commander-ng-local;
        };
      };

      # nix run .#sandbox (Linux-only: bubblewrap, Synapse, PostgreSQL, etc.)
      apps.${linuxSystem}.sandbox = {
        type = "app";
        program = "${sandboxScript}/bin/mc-sandbox";
      };

      devShells.${linuxSystem}.default = pkgs.mkShell {
        name = "matrix-commander-equalization";

        buildInputs = devPackages;

        shellHook = ''
          export PROJECT_ROOT="$(pwd)"
          export DATA_DIR="$PROJECT_ROOT/.dev-data"

          # -- Postgres --
          export PGDATA="$DATA_DIR/postgres"
          export PGHOST="$DATA_DIR/postgres"
          export PGPORT="5432"
          export PGDATABASE="synapse"

          # -- Synapse --
          export SYNAPSE_DATA="$DATA_DIR/synapse"
          export SYNAPSE_CONFIG="$SYNAPSE_DATA/homeserver.yaml"
          export SYNAPSE_LOG_CONFIG="$SYNAPSE_DATA/log.config"
          export SYNAPSE_SERVER_NAME="localhost"
          export SYNAPSE_PORT="8008"

          # -- Rust build --
          export CARGO_HOME="$DATA_DIR/cargo"
          export OPENSSL_DIR="${pkgs.openssl.dev}"
          export OPENSSL_LIB_DIR="${pkgs.lib.getLib pkgs.openssl}/lib"
          export PKG_CONFIG_PATH="${pkgs.openssl.dev}/lib/pkgconfig:${pkgs.sqlite.dev}/lib/pkgconfig:''${PKG_CONFIG_PATH:-}"

          # -- Browser testing --
          export PLAYWRIGHT_BROWSERS_PATH="${pkgs.playwright-driver.browsers}"
          export PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD="1"
          export ELEMENT_WEB_PORT="8090"
          export CINNY_PORT="8091"
          export ELEMENT_WEB_URL="http://localhost:8090"
          export CINNY_URL="http://localhost:8091"
          export NGINX_CONFIG_TEMPLATE="${nginxConfigTemplate}"

          mkdir -p "$DATA_DIR"

          # ---------- helper: sandbox ----------
          sandbox() {
            exec ${sandboxScript}/bin/mc-sandbox "$PROJECT_ROOT"
          }

          # ---------- helper: init-postgres ----------
          init-postgres() {
            if [ ! -d "$PGDATA/base" ]; then
              echo "Initializing PostgreSQL data directory..."
              initdb --no-locale --encoding=UTF8 -D "$PGDATA"
              printf "unix_socket_directories = '%s'\n" "$PGDATA" >> "$PGDATA/postgresql.conf"
              printf "listen_addresses = ${"''"}\n" >> "$PGDATA/postgresql.conf"
              printf "port = %s\n" "$PGPORT" >> "$PGDATA/postgresql.conf"
            fi
          }

          # ---------- helper: start-postgres ----------
          start-postgres() {
            init-postgres
            if ! pg_isready -q 2>/dev/null; then
              echo "Starting PostgreSQL..."
              pg_ctl -D "$PGDATA" -l "$DATA_DIR/postgres.log" start -w
              createuser --no-superuser --no-createdb --no-createrole synapse 2>/dev/null || true
              createdb --owner=synapse synapse 2>/dev/null || true
            else
              echo "PostgreSQL already running."
            fi
          }

          # ---------- helper: stop-postgres ----------
          stop-postgres() {
            if pg_isready -q 2>/dev/null; then
              echo "Stopping PostgreSQL..."
              pg_ctl -D "$PGDATA" stop
            fi
          }

          # ---------- helper: init-synapse ----------
          init-synapse() {
            if [ ! -f "$SYNAPSE_CONFIG" ]; then
              echo "Generating Synapse config..."
              mkdir -p "$SYNAPSE_DATA"
              synapse_homeserver \
                --server-name "$SYNAPSE_SERVER_NAME" \
                --config-path "$SYNAPSE_CONFIG" \
                --data-directory "$SYNAPSE_DATA" \
                --generate-config \
                --report-stats=no

              sed -e "s|__PGDATA__|$PGDATA|g" \
                  -e "s|__SYNAPSE_PORT__|$SYNAPSE_PORT|g" \
                  ${synapseOverrideTemplate} > "$SYNAPSE_DATA/homeserver-override.yaml"
            fi
          }

          # ---------- helper: start-synapse ----------
          start-synapse() {
            start-postgres
            init-synapse
            echo "Starting Synapse on port $SYNAPSE_PORT..."
            synapse_homeserver \
              --config-path "$SYNAPSE_CONFIG" \
              --config-path "$SYNAPSE_DATA/homeserver-override.yaml" \
              -D
            sleep 1
            pgrep -f 'synapse.app.homeserver' > "$SYNAPSE_DATA/synapse.pid" 2>/dev/null || true
            echo "Synapse running at http://127.0.0.1:$SYNAPSE_PORT"
          }

          # ---------- helper: stop-synapse ----------
          stop-synapse() {
            if [ -f "$SYNAPSE_DATA/synapse.pid" ]; then
              echo "Stopping Synapse..."
              kill "$(cat "$SYNAPSE_DATA/synapse.pid")" 2>/dev/null || true
              rm -f "$SYNAPSE_DATA/synapse.pid"
            fi
          }

          # ---------- helper: start-nginx ----------
          start-nginx() {
            mkdir -p "$DATA_DIR/nginx/tmp"
            sed "s|__DATA_DIR__|$DATA_DIR|g" "$NGINX_CONFIG_TEMPLATE" > "$DATA_DIR/nginx/nginx.conf"
            nginx -c "$DATA_DIR/nginx/nginx.conf"
            echo "Element Web: http://127.0.0.1:$ELEMENT_WEB_PORT"
            echo "Cinny: http://127.0.0.1:$CINNY_PORT"
          }

          # ---------- helper: stop-nginx ----------
          stop-nginx() {
            if [ -f "$DATA_DIR/nginx/nginx.pid" ]; then
              kill "$(cat "$DATA_DIR/nginx/nginx.pid")" 2>/dev/null || true
            fi
          }

          # ---------- helper: register-user ----------
          register-user() {
            local username="''${1:?usage: register-user <username> [password]}"
            local password="''${2:-$username}"
            register_new_matrix_user \
              -u "$username" \
              -p "$password" \
              -c "$SYNAPSE_CONFIG" \
              --no-admin \
              "http://127.0.0.1:$SYNAPSE_PORT"
          }

          # ---------- helper: register-admin ----------
          register-admin() {
            local username="''${1:?usage: register-admin <username> [password]}"
            local password="''${2:-$username}"
            register_new_matrix_user \
              -u "$username" \
              -p "$password" \
              -c "$SYNAPSE_CONFIG" \
              --admin \
              "http://127.0.0.1:$SYNAPSE_PORT"
          }

          # ---------- helper: start-all ----------
          start-all() {
            start-synapse
            start-nginx
            echo ""
            echo "=== Environment ready ==="
            echo "  Synapse:     http://127.0.0.1:$SYNAPSE_PORT"
            echo "  Postgres:    $PGDATA (unix socket)"
            echo "  Element Web: http://127.0.0.1:$ELEMENT_WEB_PORT"
            echo "  Cinny:       http://127.0.0.1:$CINNY_PORT"
            echo ""
            echo "  register-user <name> [password]   - create a test user"
            echo "  register-admin <name> [password]   - create an admin user"
            echo "  stop-all                          - shut everything down"
          }

          # ---------- helper: stop-all ----------
          stop-all() {
            stop-nginx
            stop-synapse
            stop-postgres
          }

          # ---------- helper: clean-all ----------
          clean-all() {
            stop-all
            echo "Removing all dev data..."
            rm -rf "$DATA_DIR"
          }

          # ---------- helper: build-rs ----------
          build-rs() {
            echo "Building matrix-commander-ng..."
            cargo build --manifest-path "$PROJECT_ROOT/matrix-commander-rs/Cargo.toml" "''${@}"
          }

          echo ""
          echo "=== matrix-commander equalization dev shell ==="
          echo ""
          echo "Available commands:"
          echo "  sandbox         - Enter isolated bubblewrap sandbox (fish)"
          echo "  start-all       - Start Postgres + Synapse + Nginx (unsandboxed)"
          echo "  stop-all        - Stop everything"
          echo "  clean-all       - Stop everything and delete dev data"
          echo "  start-nginx     - Start nginx (Element Web + Cinny)"
          echo "  stop-nginx      - Stop nginx"
          echo "  register-user   - Register a test user on Synapse"
          echo "  register-admin  - Register an admin user on Synapse"
          echo "  build-rs        - Build matrix-commander-ng"
          echo ""
          echo "Or run directly:  nix run .#sandbox"
          echo ""
        '';
      };
    };
}
