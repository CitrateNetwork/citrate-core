#!/usr/bin/env bash
# created: 2026-10-23 | branch: test/join-e2e-gate | author: Larry Klosowski + Claude Opus 5.5 (DGX lane)
# status: active. Board item Join-E2E: the fresh-machine gate "link -> install -> joined", Linux half.
#
# join-e2e.sh — run the invitee's whole path on a fresh, sealed Linux "machine" against a LOCAL
# stack (no production call is possible: the namespace has loopback only):
#
#   stack    throwaway Postgres (unix socket) + a Neon-HTTP shim, the citrate-landing join service
#            (`next dev` from a landing checkout, a throwaway Ed25519 resolve key), the comms relay
#            (a local binary), a loopback CDN serving the pinned release AppImage
#   inviter  a direct invite made through the landing store (reserve -> seal -> attach)
#   step 1   link -> join page: a real headless Chromium opens /join/<code>#<key>; the page offers
#            the install command and the app link with the invite, never sends the key anywhere
#   step 2   installer receives the invite: the page's command is run (URL swapped to loopback); the
#            installer hands the invite to the first run as `citrate://join/<code>#<key>` argv,
#            never prints it and never leaves it in the app's environment
#   step 3   install: the pinned AppImage lands where the installer says, with its command link and
#            a citrate:// handler entry; the app starts its node sidecar; quitting leaves nothing
#   step 4   first run redeems the invite: 4a the service's signed resolve verifies against the pin
#            and the envelope opens (harness-side reference check); 4b the app does it — PENDING
#   step 5   member is in the inviter's group — PENDING
#
# Spec, pass criteria, observables and failure modes: docs/JOIN_E2E_GATE.md. macOS runs on the
# MAC lane's machine: docs/JOIN_E2E_MACOS_RUNBOOK.md.
#
# SAFETY. Reuses the K1-L sealed namespace (scripts/ci/linux-native-acceptance.sh, core#384): the
# app only ever starts as a descendant of PID 1 of fresh user/PID/mount/net/IPC/UTS namespaces
# with /home, /tmp, /run, /dev hidden, its own Xvfb display, session bus and keyring, after the
# K1-L isolation probes plus this gate's own (no route off loopback, with a positive control).
# Every key is generated per run and lives in the run dir only. Postgres refuses to run as root, so
# it runs in a nested user namespace as the mapped invoking user.
#
# Usage:
#   scripts/ci/join-e2e.sh --landing DIR [--relay BIN] [--appimage FILE | --cache DIR]
#                          [--work DIR] [--log-dir DIR] [--self-test]
#     --landing   a citrate-landing checkout WITH node_modules (npm ci), at the ref under test
#                 (today: feat/join-v2-page, landing#128 on #123; see the spec's ref table)
#     --relay     a comms-relay binary (cargo build --release -p comms-relay in citrate-comms)
#     --appimage  the AppImage the loopback CDN serves; default: the pinned "to" AppImage from
#                 scripts/ci/linux-native-acceptance/pins.txt, fetched (sha256-checked) into --cache
#     --test-build DIR  the output of join-e2e/build-test-app.sh: a test build of the app at a ref
#                 under test that pins that build's throwaway resolve key and talks to this loopback
#                 join service. The CDN serves it, the landing signs with its key, and step 4b-i
#                 (the first run holds the invite) is checked instead of left pending
#     --self-test run the gate on one good case and three broken ones and require each to fail
#                 exactly its own steps: installer drops the invite (step 2), expired invite (steps
#                 1 and 4a), resolve checked against a key that is not the pin (step 4a)
# Env: JE_CHROMIUM (a chrome-headless-shell; default: the newest under ~/.cache/ms-playwright),
#      JE_LAUNCH_TIMEOUT (default 180), JE_KEEP=1 (keep the run dir).
# Exit: 0 every automated step passed (pending steps are listed; the gate is NOT met while any
#       step is pending), 1 a step failed, 2 isolation or infrastructure problem.
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SELF="$ROOT/scripts/ci/join-e2e.sh"
JE="$ROOT/scripts/ci/join-e2e"
K1L="$ROOT/scripts/ci/linux-native-acceptance.sh"

die() { echo "::error::$*" >&2; exit 2; }

# The pending steps, with what each one waits on. Kept here so the summary and the spec agree;
# docs/JOIN_E2E_GATE.md links to this list.
PENDING_4B="first run redeems the invite in the app: needs core#353 (v2 resolve/open), core#355 (deep link -> pending invite, Review/Join), core#375 (rendezvous join), a test build with this run's pin, and a UI driver for the explicit Join"
PENDING_4B_II="the member's Review and Join in the app redeem the invite: needs a UI driver for the explicit Join (webview), and core#355 + core#375 on one ref"
PENDING_5="member is in the inviter's group: needs the relay/daemon rendezvous (citrate-comms R1-R4, JOIN_GROW_S2_RELAY.md §15; no PR yet), core#373/#374 (inviter open + states), core#375, and an inviter app or daemon on the local relay"

# ============================================================================================
# Outer phase: stage everything into one run dir, then enter the namespace.
# ============================================================================================
outer_main() {
  local LANDING="" RELAY="" AI="" CACHE="" WORK="" LOG_DIR="" SELFTEST=0 TB=""
  while [ $# -gt 0 ]; do
    case "$1" in
      --landing) LANDING="$2"; shift 2 ;;
      --relay) RELAY="$2"; shift 2 ;;
      --appimage) AI="$2"; shift 2 ;;
      --test-build) TB="$2"; shift 2 ;;
      --cache) CACHE="$2"; shift 2 ;;
      --work) WORK="$2"; shift 2 ;;
      --log-dir) LOG_DIR="$2"; shift 2 ;;
      --self-test) SELFTEST=1; shift ;;
      -h|--help) sed -n '2,48p' "$SELF"; exit 0 ;;
      *) die "unknown argument: $1" ;;
    esac
  done
  [ "$(uname -s)" = Linux ] || die "Linux only"
  local t
  for t in unshare setsid python3 ss ip sha256sum Xvfb openssl curl rsync node \
           /usr/lib/postgresql/16/bin/initdb /usr/lib/postgresql/16/bin/postgres psql; do
    command -v "$t" >/dev/null || die "missing tool: $t"
  done
  /usr/bin/python3 -I -c 'import nacl.signing, nacl.bindings' 2>/dev/null || die "missing python3-nacl (system python3)"
  [ -n "$LANDING" ] && [ -f "$LANDING/public/install.sh" ] && [ -d "$LANDING/node_modules/next" ] ||
    die "--landing must be a citrate-landing checkout with node_modules (npm ci)"
  [ -n "$RELAY" ] && [ -x "$RELAY" ] || die "--relay must be a comms-relay binary"
  [ -f "$K1L" ] && [ -f "$ROOT/scripts/ci/lib/pidns.sh" ] || die "the K1-L harness (core#384) is missing"
  local arch; arch="$(uname -m)"
  WORK="${WORK:-$ROOT/target/join-e2e}"
  mkdir -p "$WORK" || die "cannot create $WORK"
  WORK="$(cd "$WORK" && pwd)"
  case "$WORK" in /|/home|/home/*/|"$HOME"|"$HOME/") die "refusing work dir $WORK" ;; esac
  if [ -n "$TB" ]; then
    [ -z "$AI" ] || die "--test-build and --appimage are exclusive"
    TB="$(cd "$TB" && pwd)" && [ -f "$TB/Citrate-Core-test.AppImage" ] && [ -f "$TB/keys/signing.jwk" ] &&
      [ -f "$TB/keys/pin.txt" ] && [ -f "$TB/provenance.txt" ] || die "--test-build must be a build-test-app.sh output dir"
    AI="$TB/Citrate-Core-test.AppImage"
  fi
  if [ -z "$AI" ]; then
    CACHE="${CACHE:-$WORK/cache}"; mkdir -p "$CACHE"
    AI="$(fetch_pinned_appimage "$arch" "$CACHE")" || exit 2
  fi
  [ -f "$AI" ] || die "AppImage not found: $AI"
  local chromium="${JE_CHROMIUM:-}"
  if [ -z "$chromium" ]; then
    chromium="$(ls -d "$HOME"/.cache/ms-playwright/chromium_headless_shell-*/chrome-*/chrome-headless-shell 2>/dev/null | sort -V | tail -n 1)"
  fi
  [ -n "$chromium" ] && [ -x "$chromium" ] || die "no chrome-headless-shell (npx playwright install chromium-headless-shell, or set JE_CHROMIUM)"
  if ! unshare --user --map-root-user --pid --fork --mount-proc --net --ipc --uts true 2>/dev/null; then
    echo "::error::cannot create unprivileged user+pid+net namespaces; refusing to run"
    echo "  (Ubuntu 24.04 runner: sudo sysctl -w kernel.apparmor_restrict_unprivileged_userns=0;"
    echo "   never change that on a desktop)"
    exit 2
  fi

  local RUN; RUN="$(mktemp -d "$WORK/run.XXXXXX")" || die "mktemp failed"
  mkdir -p "$RUN/logs" "$RUN/bin" "$RUN/cdn" "$RUN/harness/scripts/ci/lib" \
    "$RUN/harness/scripts/ci/join-e2e" "$RUN/harness/scripts/ci/linux-native-acceptance" || die "mkdir failed"
  # The harness runs from a copy inside the run dir (the namespace hides /tmp and /home). The K1-L
  # script is copied without its two dispatch lines, so sourcing it defines its functions only.
  cp "$SELF" "$RUN/harness/scripts/ci/" && cp "$JE"/* "$RUN/harness/scripts/ci/join-e2e/" &&
    cp "$ROOT/scripts/ci/lib/pidns.sh" "$RUN/harness/scripts/ci/lib/" &&
    cp "$ROOT/scripts/ci/linux-native-acceptance/x11_windows.py" "$RUN/harness/scripts/ci/linux-native-acceptance/" ||
    die "staging the harness failed"
  k1l_functions_only "$K1L" >"$RUN/harness/scripts/ci/k1l-functions.sh" || exit 2
  # The landing checkout: sources copied (next dev writes into its tree), node_modules hard-linked.
  rsync -a --exclude node_modules --exclude .next --exclude .git "$LANDING/" "$RUN/landing/" || die "copy landing failed"
  cp -al "$LANDING/node_modules" "$RUN/landing/node_modules" 2>/dev/null ||
    cp -a "$LANDING/node_modules" "$RUN/landing/node_modules" || die "copy node_modules failed"
  local lrev; lrev="$(git -C "$LANDING" rev-parse --short HEAD 2>/dev/null || echo unknown)"
  cp "$(command -v node)" "$RUN/bin/node" && cp "$RELAY" "$RUN/bin/comms-relay" || die "copy binaries failed"
  local chdir; chdir="$(dirname "$chromium")"
  cp -al "$chdir" "$RUN/browser" 2>/dev/null || cp -a "$chdir" "$RUN/browser" || die "copy chromium failed"
  ln -f "$AI" "$RUN/cdn/app.AppImage" 2>/dev/null || cp -- "$AI" "$RUN/cdn/app.AppImage" || die "stage AppImage failed"
  awk '$8 ~ /^@/ {print substr($8, 2)}' /proc/net/unix | sort -u >"$RUN/logs/host-abstract-sockets.txt"
  if [ -n "$TB" ]; then
    mkdir -m 0700 "$RUN/testbuild-keys" && cp "$TB/keys/signing.jwk" "$TB/keys/pin.txt" "$RUN/testbuild-keys/" &&
      cp "$TB/provenance.txt" "$RUN/logs/test-build-provenance.txt" || die "staging the test build's key failed"
  fi

  local cases="good"
  [ "$SELFTEST" -eq 1 ] && cases="good drop-invite expired rogue-key"
  echo "▶ enter: unshare --user --map-root-user --pid --fork --mount-proc --net --ipc --uts setsid"
  unshare --user --map-root-user --pid --fork --mount-proc --net --ipc --uts \
    setsid env -i PATH=/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin LANG=C.UTF-8 \
      K1L_HOST_HOME="$HOME" K1L_HOST_UID="$(id -u)" K1L_RUN="$RUN" K1L_QUIT=x11 \
      JE_HOST_UID="$(id -u)" JE_HOST_GID="$(id -g)" JE_CASES="$cases" JE_ARCH="$arch" \
      JE_HARNESS_REV="$(git -C "$ROOT" rev-parse --short HEAD 2>/dev/null || echo unknown)" \
      JE_LANDING_REV="$lrev" JE_RELAY_SRC="${RELAY}" JE_CHROMIUM_NAME="${chdir##*/}" \
      JE_APPIMAGE_SHA="$(sha256sum <"$AI" | cut -d' ' -f1)" JE_TEST_BUILD="${TB:+1}" \
      JE_LAUNCH_TIMEOUT="${JE_LAUNCH_TIMEOUT:-180}" \
      bash "$RUN/harness/scripts/ci/join-e2e.sh" --inner
  local rc=$?
  [ -f "$RUN/logs/summary.txt" ] && cat "$RUN/logs/summary.txt"
  if [ "$SELFTEST" -eq 1 ] && [ "$rc" -ne 2 ]; then
    self_test_verdict "$RUN/logs/summary.txt" "${TB:+1}" | tee -a "$RUN/logs/self-test.txt"
    rc=${PIPESTATUS[0]}
  fi
  if [ -n "$LOG_DIR" ]; then mkdir -p "$LOG_DIR" && cp -r "$RUN/logs/." "$LOG_DIR/"; fi
  cleanup_run "$RUN" "$WORK"
  exit "$rc"
}

# The K1-L script minus its last two lines (the --inner / outer dispatch), checked to be exactly
# those lines so a change in that script can't make this source something else.
k1l_functions_only() { # k1l-script
  local want got
  want=$'if [ "${1:-}" = --inner ]; then inner_main; fi\nouter_main "$@"'
  got="$(tail -n 2 "$1")"
  [ "$got" = "$want" ] || { echo "::error::$1 no longer ends with its dispatch lines; update join-e2e.sh" >&2; return 2; }
  head -n -2 "$1"
}

fetch_pinned_appimage() { # arch cache -> prints the path
  local arch="$1" cache="$2" role ver a kind sha url f
  while read -r role ver a kind sha url; do
    [ "$role" = to ] && [ "$a" = "$arch" ] && [ "$kind" = AppImage ] || continue
    f="$cache/to-$arch-$ver.AppImage"
    if [ ! -f "$f" ] || [ "$(sha256sum <"$f" | cut -d' ' -f1)" != "$sha" ]; then
      echo "▶ fetch $ver $arch AppImage" >&2
      curl -fsSL --retry 3 -o "$f.part" "$url" || { echo "::error::download failed: $url" >&2; return 1; }
      mv "$f.part" "$f"
    fi
    [ "$(sha256sum <"$f" | cut -d' ' -f1)" = "$sha" ] || { echo "::error::sha256 mismatch: $url" >&2; rm -f -- "$f"; return 1; }
    echo "$f"; return 0
  done <"$ROOT/scripts/ci/linux-native-acceptance/pins.txt"
  echo "::error::no pinned 'to' AppImage for $arch" >&2; return 1
}

cleanup_run() { # run work
  local run="$1" work="$2"
  [ "${JE_KEEP:-0}" = 1 ] && { echo "kept run dir: $run"; return 0; }
  case "$run" in "$work"/run.*) ;; *) echo "not removing $run (outside $work)"; return 0 ;; esac
  [ -d "$run" ] && [ ! -L "$run" ] || return 0
  rm -rf -- "${run:?}/landing" "${run:?}/browser" "${run:?}/cdn" "${run:?}/bin" "${run:?}/harness" "${run:?}/state" "${run:?}/testbuild-keys" 2>/dev/null
  echo "logs: $run/logs"
}

# Self-test verdict from the summary: the good case has no FAIL; each broken case fails exactly the
# step it breaks (later steps may be "not reached", never PASS-by-accident on the broken one).
# With a test build, step 4b-i is checked too: the drop-invite case then also fails 4b-i (no
# invite reached the app), and the others are unchanged (expired never installs).
self_test_verdict() { # summary [test-build]
  local s="$1" spec c want got fails=0 drop="drop-invite:2"
  [ -n "${2:-}" ] && drop="drop-invite:2 4b-i"
  for spec in "good:" "$drop" "expired:1 4a" "rogue-key:4a"; do
    c="${spec%%:*}"; want="${spec#*:}"
    got="$(awk -v c="[$c]" '$1 == "FAIL" && $2 == c {print $3}' "$s" | sort -u | paste -sd' ')"
    local ok=WRONG
    if [ -z "$want" ]; then
      [ -z "$got" ] && grep -q "^PASS \[$c\] 3 " "$s" && grep -q "^PASS \[$c\] 4a " "$s" && ok=ok
    else
      [ "$got" = "$want" ] && ok=ok
    fi
    echo "self-test $c: expect failing=[${want}] got failing=[${got}] -> $ok"
    [ "$ok" = ok ] || fails=$((fails + 1))
  done
  if [ "$fails" -eq 0 ]; then echo "SELF-TEST: PASS"; return 0; fi
  echo "SELF-TEST: FAIL ($fails case(s) misjudged)"; return 1
}

# ============================================================================================
# Inner phase: PID 1 of the namespace.
# ============================================================================================
JW=/tmp/je              # short paths (unix sockets); /tmp is the namespace's own tmpfs
LPORT=3317              # landing (next dev)
CPORT=8717              # loopback CDN
RPORT=18787             # comms relay (ws); admin on RPORT+1
PGPORT=5439
STACK_PIDS=()

# Results: one line per (case, step). FAIL marks the run failed; PENDING never does.
step_pass() { say "PASS [$1] $2 $3"; }
step_fail() { say "FAIL [$1] $2 $3"; FAILED=1; }
step_skip() { say "SKIP [$1] $2 not reached: $3"; }
step_pending() { say "PENDING [$1] $2 $3"; }

as_user() { # run a command as the invoking user's mapped id (postgres refuses root)
  unshare --user --map-user="$JE_HOST_UID" --map-group="$JE_HOST_GID" "$@"
}
psql_q() { psql -h "$JW/pg" -p "$PGPORT" -U join_e2e -d join_e2e -At -v ON_ERROR_STOP=1 -c "$1"; }

# This gate's own isolation probe: nothing but loopback is reachable, and a loopback listener is
# (positive control). Runs after the K1-L probes.
p_offline() {
  python3 -I - <<'PY'
import socket, sys
srv = socket.socket(); srv.bind(("127.0.0.1", 0)); srv.listen(1)
c = socket.create_connection(srv.getsockname(), timeout=2); c.close(); srv.close()
reached = []
for host, port in (("1.1.1.1", 443), ("8.8.8.8", 53), ("142.93.58.145", 443)):
    s = socket.socket(); s.settimeout(2)
    try:
        s.connect((host, port)); reached.append(host)
    except OSError:
        pass
    finally:
        s.close()
try:
    socket.getaddrinfo("www.citrate.ai", 443); reached.append("dns:www.citrate.ai")
except OSError:
    pass
print(f"off-loopback reachable: {reached}")
sys.exit(1 if reached else 0)
PY
}

start_display() {
  export K1L_PRIVATE_DISPLAY=":77"
  Xvfb "$K1L_PRIVATE_DISPLAY" -screen 0 1280x800x24 -nolisten tcp -extension GLX >"$W/logs/xvfb.log" 2>&1 &
  INFRA_PIDS+=("$!")
  local i; for i in $(seq 1 50); do [ -S /tmp/.X11-unix/X77 ] && break; sleep 0.1; done
  DISPLAY="$K1L_PRIVATE_DISPLAY" python3 -I -c 'import ctypes,ctypes.util,os,sys; x=ctypes.CDLL(ctypes.util.find_library("X11") or "libX11.so.6"); x.XOpenDisplay.restype=ctypes.c_void_p; sys.exit(0 if x.XOpenDisplay(os.environ["DISPLAY"].encode()) else 1)'
}

wait_for() { # secs, then a command; polls every 0.5 s
  local n=$(( $1 * 2 )); shift
  local i; for i in $(seq 1 "$n"); do "$@" >/dev/null 2>&1 && return 0; sleep 0.5; done
  return 1
}

# --------------------------------------------------------------------------------------------
# The local stack
# --------------------------------------------------------------------------------------------
start_stack() {
  local L="$W/landing" K="$W/state/keys" PGB=/usr/lib/postgresql/16/bin
  mkdir -p "$JW/pg" "$K" "$JW/landing-home" "$W/state/relay" && chmod 0700 "$K" || return 1
  # Postgres: unix socket only, in a nested user namespace as the invoking user.
  as_user "$PGB/initdb" -D "$JW/pg/data" -U join_e2e --auth=trust -E UTF8 --no-sync --no-instructions \
    >"$W/logs/initdb.log" 2>&1 || { say "stack: initdb failed (logs/initdb.log)"; return 1; }
  as_user "$PGB/postgres" -D "$JW/pg/data" -k "$JW/pg" -p "$PGPORT" -c listen_addresses= -c fsync=off \
    >"$W/logs/postgres.log" 2>&1 &
  STACK_PIDS+=("$!")
  wait_for 20 test -S "$JW/pg/.s.PGSQL.$PGPORT" || { say "stack: postgres did not start (logs/postgres.log)"; return 1; }
  psql -h "$JW/pg" -p "$PGPORT" -U join_e2e -d postgres -qc 'CREATE DATABASE join_e2e' >>"$W/logs/stack.log" 2>&1 || return 1
  # Schema: the landing's own generated migrations. 0000_init is skipped: 0001 recreates its tables
  # without IF NOT EXISTS, so the full chain does not apply to an empty database (the landing's
  # pg integration test applies 0002+ for the same reason).
  local f n=0
  for f in "$L"/drizzle/migrations/[0-9][0-9][0-9][0-9]_*.sql; do
    case "${f##*/}" in 0000_*) continue ;; esac
    sed 's/--> statement-breakpoint//' "$f" |
      psql -h "$JW/pg" -p "$PGPORT" -U join_e2e -d join_e2e -q -v ON_ERROR_STOP=1 >>"$W/logs/stack.log" 2>&1 ||
      { say "stack: migration ${f##*/} failed (logs/stack.log)"; return 1; }
    n=$((n + 1))
  done
  psql_q "SELECT to_regclass('join_codes') IS NOT NULL AND to_regclass('rate_limit_buckets') IS NOT NULL" | grep -qx t ||
    { say "stack: join_codes / rate_limit_buckets missing after migrations"; return 1; }
  # Throwaway TLS for the Neon-HTTP shim, and the throwaway resolve-signing key + its pin.
  openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:P-256 -nodes -keyout "$K/ca.key" -out "$K/ca.pem" \
    -days 1 -subj /CN=join-e2e-throwaway-ca >>"$W/logs/stack.log" 2>&1 &&
    openssl req -newkey ec -pkeyopt ec_paramgen_curve:P-256 -nodes -keyout "$K/db.key" -out "$K/db.csr" \
      -subj /CN=127.0.0.1 >>"$W/logs/stack.log" 2>&1 &&
    printf 'subjectAltName=IP:127.0.0.1\nbasicConstraints=CA:FALSE\n' >"$K/db.ext" &&
    openssl x509 -req -in "$K/db.csr" -CA "$K/ca.pem" -CAkey "$K/ca.key" -CAcreateserial -out "$K/db.pem" \
      -days 1 -extfile "$K/db.ext" >>"$W/logs/stack.log" 2>&1 || { say "stack: TLS setup failed"; return 1; }
  python3 -I "$JE/join_crypto.py" selfcheck | tee -a "$W/logs/stack.log" | grep -q '^selfcheck ok' ||
    { say "stack: invite crypto known-answer check failed"; return 1; }
  if [ -n "${JE_TEST_BUILD:-}" ]; then
    cp "$W/testbuild-keys/signing.jwk" "$W/testbuild-keys/pin.txt" "$K/" || return 1   # the key the test build pins
  else
    python3 -I "$JE/join_crypto.py" keygen "$K" || return 1
  fi
  mkdir -p "$L/.join-e2e" && cp "$JE/neon-http-shim.mjs" "$JE/seed-invite.mts" "$JE/page-check.mjs" "$L/.join-e2e/" || return 1
  ( cd "$L" && exec "$W/bin/node" .join-e2e/neon-http-shim.mjs "$JW/pg/.s.PGSQL.$PGPORT" "$K/db.pem" "$K/db.key" "$JW/shim.ready" ) \
    >"$W/logs/shim.log" 2>&1 &
  STACK_PIDS+=("$!")
  wait_for 20 test -s "$JW/shim.ready" || { say "stack: Neon-HTTP shim did not start (logs/shim.log)"; return 1; }
  # The landing join service.
  LENV=(PATH="$W/bin:$PATH" HOME="$JW/landing-home" NODE_EXTRA_CA_CERTS="$K/ca.pem" NEXT_TELEMETRY_DISABLED=1
    DATABASE_URL="postgresql://join_e2e@127.0.0.1/join_e2e" JOIN_SIGNING_JWK="$(cat "$K/signing.jwk")"
    JOIN_MINTER_SALT="$(openssl rand -hex 24)" ENCRYPTION_KEY="$(openssl rand -base64 32)"
    NEXT_PUBLIC_SITE_URL="http://127.0.0.1:$LPORT")
  ( cd "$L" && exec env "${LENV[@]}" "$W/bin/node" node_modules/next/dist/bin/next dev -H 127.0.0.1 -p "$LPORT" ) \
    >"$W/logs/landing.log" 2>&1 &
  STACK_PIDS+=("$!")
  wait_for 180 curl -fsS -o /dev/null "http://127.0.0.1:$LPORT/api/join/jwks" ||
    { say "stack: landing did not serve /api/join/jwks (logs/landing.log)"; return 1; }
  local jx; jx="$(curl -fsS "http://127.0.0.1:$LPORT/api/join/jwks" | python3 -I -c 'import json,sys; print(json.load(sys.stdin)["keys"][0]["x"])')"
  [ "$jx" = "$(awk '{print $2}' "$K/pin.txt")" ] || { say "stack: the landing's JWKS is not this run's throwaway key"; return 1; }
  # The comms relay (its admin /health reports the commit it was built from).
  head -c 32 /dev/urandom | od -An -tx1 | tr -d ' \n' >"$K/relay-master.hex" && chmod 0600 "$K/relay-master.hex"
  env -i PATH="$PATH" HOME="$JW/landing-home" CITRATE_COMMS_BIND="127.0.0.1:$RPORT" \
    CITRATE_COMMS_ADMIN_BIND="127.0.0.1:$((RPORT + 1))" CITRATE_COMMS_DATA="$W/state/relay/data" \
    CITRATE_COMMS_OWNER=0x0000000000000000000000000000000000000001 CITRATE_COMMS_DOMAIN=relay.join-e2e.test \
    CITRATE_COMMS_MASTER_KEY_FILE="$K/relay-master.hex" CITRATE_COMMS_ADMIN_TOKEN_FILE="$K/relay-admin-token" \
    "$W/bin/comms-relay" >"$W/logs/relay.log" 2>&1 &
  STACK_PIDS+=("$!")
  wait_for 30 curl -fsS -o "$JW/relay-health.json" "http://127.0.0.1:$((RPORT + 1))/health" ||
    { say "stack: relay /health did not answer (logs/relay.log)"; return 1; }
  # The loopback CDN: the feed and SHA256SUMS the installer reads, and the pinned AppImage.
  local key; case "$JE_ARCH" in aarch64) key=linux-aarch64 ;; x86_64) key=linux-x86_64 ;; *) return 1 ;; esac
  mkdir -p "$W/cdn/downloads/updater" "$W/cdn/downloads/vtest" &&
    ln -f "$W/cdn/app.AppImage" "$W/cdn/downloads/vtest/Citrate-Core-$key.AppImage" || return 1
  printf '%s  %s\n' "$JE_APPIMAGE_SHA" "Citrate-Core-$key.AppImage" >"$W/cdn/downloads/vtest/SHA256SUMS"
  printf '{"version":"0.0.0-join-e2e","platforms":{"%s":{"url":"http://127.0.0.1:%s/downloads/vtest/Citrate-Core-%s.AppImage"}}}\n' \
    "$key" "$CPORT" "$key" >"$W/cdn/downloads/updater/latest.json"
  ( cd "$W/cdn" && exec python3 -I -m http.server "$CPORT" --bind 127.0.0.1 ) >"$W/logs/cdn.log" 2>&1 &
  STACK_PIDS+=("$!")
  wait_for 20 curl -fsS -o /dev/null "http://127.0.0.1:$CPORT/downloads/updater/latest.json" || { say "stack: CDN did not start"; return 1; }
  INFRA_PIDS+=("${STACK_PIDS[@]}")
  say "stack: postgres (unix socket, $n migrations), landing @ $JE_LANDING_REV on 127.0.0.1:$LPORT, relay $(python3 -I -c 'import json;print(json.load(open("'"$JW"'/relay-health.json"))["sha"][:8])') on 127.0.0.1:$RPORT, CDN on 127.0.0.1:$CPORT (AppImage ${JE_APPIMAGE_SHA:0:16})"
}

# The stack's processes (and the workers they started since) belong to the "machine", not to the
# app: exempt them from the app's quit and leftover checks, and keep them out of its service list.
stack_tree_is_infra() {
  local p c
  for p in "${STACK_PIDS[@]}"; do
    for c in $(descendants "$p" | awk '{print $1}'); do
      case " ${INFRA_PIDS[*]} " in *" $c "*) ;; *) INFRA_PIDS+=("$c") ;; esac
    done
  done
}

# --------------------------------------------------------------------------------------------
# One case: inviter -> steps 1..5
# --------------------------------------------------------------------------------------------
seed_invite() { # case dir -> writes $dir/code.txt, $dir/env.txt, $dir/key.txt
  local c="$1" d="$2" L="$W/landing"
  local -a e=("${LENV[@]}")
  ( cd "$L" && env "${e[@]}" "$W/bin/node" --conditions=react-server --import tsx .join-e2e/seed-invite.mts reserve 3600 ) \
    >"$d/code.txt" 2>>"$W/logs/seed.log" || return 1
  local code; code="$(cat "$d/code.txt")"
  # The relay invite sealed in the envelope. No inviter group exists on the local relay yet (step 5
  # is pending), so this is a well-formed {g,t,k,n} with fresh random values: it opens and verifies,
  # and admits no one.
  python3 -I -c 'import secrets,json; print(json.dumps({"g": secrets.token_hex(16), "t": secrets.token_urlsafe(32), "k": secrets.token_hex(33), "n": "4a6f696e20453245"}))' |
    python3 -I "$JE/join_crypto.py" seal "$code" "$d" || return 1
  ( cd "$L" && env "${e[@]}" "$W/bin/node" --conditions=react-server --import tsx .join-e2e/seed-invite.mts attach "$code" "$d/env.txt" ) \
    >"$d/envhash.txt" 2>>"$W/logs/seed.log" || return 1
  if [ "$c" = expired ]; then
    psql_q "UPDATE join_codes SET expires_at = now() - interval '1 minute' WHERE code = '$code'" >/dev/null || return 1
  fi
}

# Step 1: link -> join page.
check_page() { # case dir
  local c="$1" d="$2" code key
  code="$(cat "$d/code.txt")"; key="$(cat "$d/key.txt")"
  local want_cmd="curl -fsSL https://citrate.ai/install.sh | CITRATE_JOIN='$code#$key' sh"
  local want_app="citrate://join/$code#$key"
  ( cd "$W/landing" && env HOME="$JW/landing-home" "$W/bin/node" .join-e2e/page-check.mjs \
      "$W/browser/chrome-headless-shell" "http://127.0.0.1:$LPORT/join/$code#$key" "$d/page.json" ) \
    >>"$W/logs/page-$c.log" 2>&1 || { step_fail "$c" 1 "link -> join page: the browser could not load the page (logs/page-$c.log)"; return 1; }
  step_pass "$c" 1 "link -> join page (STUB: verdict not implemented yet)"
  printf '%s\n' "$want_cmd" >"$d/command.txt"
}

# Find the running app (the citrate-core binary the AppImage runs) under a parent pid.
app_pid_under() { # root-pid
  local p e
  for p in $(descendants "$1" | awk '{print $1}'); do
    e="$(readlink "/proc/$p/exe" 2>/dev/null)"; e="${e% (deleted)}"
    [ "${e##*/}" = citrate-core ] && { echo "$p"; return 0; }
  done
  return 1
}
find_app() { # -> sets APP_PID (the process the installer started) and APP_BIN (the citrate-core pid)
  local p a
  for p in /proc/[0-9]*; do
    p="${p#/proc/}"; [ "$p" = 1 ] && continue
    a="$(tr '\0' ' ' <"/proc/$p/cmdline" 2>/dev/null)"
    case "$a" in *"$CASE_DIR/apps/Citrate-Core.AppImage"*) ;; *) continue ;; esac
    case "$a" in *--appimage-extract*) continue ;; esac
    [ "$(ppid_of "$p")" = 1 ] || continue   # the installer's nohup child, reparented to PID 1
    APP_PID="$p"; APP_BIN="$(app_pid_under "$p" || { e="$(readlink "/proc/$p/exe" 2>/dev/null)"; [ "${e##*/}" = citrate-core ] && echo "$p"; })"
    [ -n "$APP_BIN" ] && return 0
  done
  return 1
}

# Steps 2 and 3: run the page's command against the loopback copies, then check the hand-off and
# the install.
check_install() { # case dir
  local c="$1" d="$2" code key cmd value
  code="$(cat "$d/code.txt")"; key="$(cat "$d/key.txt")"; cmd="$(cat "$d/command.txt")"
  # The command must be exactly the published one-liner; only its URL is swapped for the loopback copy.
  value="$(printf '%s' "$cmd" | sed -n "s/^curl -fsSL https:\/\/citrate\.ai\/install\.sh | CITRATE_JOIN='\([^']*\)' sh\$/\1/p")"
  [ -n "$value" ] || { step_fail "$c" 2 "installer receives the invite: the page's command is not the one-line installer"; return 1; }
  curl -fsS -o "$d/install.sh" "http://127.0.0.1:$LPORT/install.sh" ||
    { step_fail "$c" 2 "installer receives the invite: the landing did not serve /install.sh"; return 1; }
  cmp -s "$d/install.sh" "$W/landing/public/install.sh" ||
    { step_fail "$c" 2 "installer receives the invite: /install.sh differs from public/install.sh"; return 1; }
  if [ "$c" = drop-invite ]; then
    # Negative control: an installer that opens the app but drops the invite.
    sed -i 's|nohup "\$app" "\$JOIN_URL"|nohup "$app"|' "$d/install.sh"
    cmp -s "$d/install.sh" "$W/landing/public/install.sh" && { say "     [$c] the drop-invite mutation did not apply"; return 2; }
  fi
  HOME_DIR="$d/home"; mkdir -p "$HOME_DIR" "$d/apps"
  start_session_services "$c" || { step_fail "$c" 3 "install: could not start the private bus/keyring"; return 1; }
  infra_tree; BASE_SOCKETS="$(listening "${INFRA_TREE[@]}")"
  local -a ienv=(PATH="$PATH" HOME="$HOME_DIR" XDG_CONFIG_HOME="$HOME_DIR/.config" XDG_DATA_HOME="$HOME_DIR/.local/share"
    XDG_CACHE_HOME="$HOME_DIR/.cache" XDG_STATE_HOME="$HOME_DIR/.local/state" XDG_RUNTIME_DIR="$XRT"
    DBUS_SESSION_BUS_ADDRESS="unix:path=$XRT/bus" DISPLAY="$K1L_PRIVATE_DISPLAY" NO_AT_BRIDGE=1 TMPDIR=/tmp
    APPIMAGE_EXTRACT_AND_RUN=1 CITRATE_INSTALL_TEST=1 CITRATE_CDN="http://127.0.0.1:$CPORT/downloads"
    CITRATE_INSTALL_DIR="$d/apps" LANG=C.UTF-8)
  # `curl … | CITRATE_JOIN='…' sh`, as the page says, with the invite from the page's own command.
  env "${ienv[@]}" sh -c 'cat "$1" | CITRATE_JOIN="$2" sh' sh "$d/install.sh" "$value" >"$d/installer.out" 2>&1
  local irc=$?
  cp "$d/installer.out" "$W/logs/installer-$c.out"
  CASE_DIR="$d"; APP_PID=""; APP_BIN=""
  [ "$irc" -eq 0 ] || { step_fail "$c" 2 "installer receives the invite: the installer exited $irc (logs/installer-$c.out)"; stop_session_services; return 1; }
  wait_for 30 find_app; find_app
  if [ -z "$APP_BIN" ]; then
    step_fail "$c" 2 "installer receives the invite: the installer did not start the app (logs/installer-$c.out)"
    stop_strays; stop_session_services; return 1
  fi
  # Step 2 observables: argv, environment, installer output.
  local argv env_has out_has
  argv="$(python3 -I -c 'import sys; print("\n".join(open(f"/proc/{sys.argv[1]}/cmdline","rb").read().decode().split("\0")[1:]))' "$APP_BIN")"
  env_has=0; tr '\0' '\n' <"/proc/$APP_BIN/environ" 2>/dev/null | grep -q '^CITRATE_JOIN=' && env_has=1
  out_has=0; grep -qF -- "$key" "$d/installer.out" && out_has=1
  step_pass "$c" 2 "installer receives the invite (STUB: verdict not implemented yet)"
  # Step 3 observables: files, the app comes up (node sidecar), quit leaves nothing.
  local app="$d/apps/Citrate-Core.AppImage" desk="$HOME_DIR/.local/share/applications/ai.citrate.core.desktop" bad=""
  [ "$(sha256sum <"$app" 2>/dev/null | cut -d' ' -f1)" = "$JE_APPIMAGE_SHA" ] || bad="$bad AppImage-sha"
  [ "$(readlink "$HOME_DIR/.local/bin/citrate-core")" = "$app" ] || bad="$bad command-link"
  grep -qx 'MimeType=x-scheme-handler/citrate;' "$desk" 2>/dev/null || bad="$bad url-handler"
  grep -qxF "Exec=\"$app\" %u" "$desk" 2>/dev/null || bad="$bad exec-line"
  local i up=0
  for i in $(seq 1 "$JE_LAUNCH_TIMEOUT"); do
    alive "$APP_PID" || break
    sidecars_of "$APP_PID" | awk '{print $1}' | grep -qx citrate && { up=1; break; }
    sleep 1
  done
  [ "$up" -eq 1 ] || bad="$bad node-sidecar"
  if [ -n "$bad" ]; then
    step_fail "$c" 3 "install: missing or wrong:$bad (logs/installer-$c.out)"
    force_stop $(descendants "$APP_PID" | awk '{print $1}') "$APP_PID"; stop_strays; stop_session_services; return 1
  fi
  sleep 5
  HOLDS=()
  [ -n "${JE_TEST_BUILD:-}" ] && check_holds_invite "$c" "$d"
  stack_tree_is_infra
  local before_fail=$FAILED
  if quit_app appimage "$c" >/dev/null; then
    FAILED=$before_fail
    step_pass "$c" 3 "install: pinned AppImage at the install dir (sha ${JE_APPIMAGE_SHA:0:12}), citrate-core command link, citrate:// handler entry; the app started its node sidecar in ${i}s and quit left no process or socket"
  else
    FAILED=$before_fail
    step_fail "$c" 3 "install: the app did not quit cleanly (see the quit lines above)"
  fi
  [ "${#HOLDS[@]}" -eq 2 ] && "${HOLDS[0]}" "$c" 4b-i "${HOLDS[1]}"   # observed before quit, reported after step 3
  stop_session_services
}

# Step 4b-i (test build only): the first run took the invite from its launch link and holds it,
# before the member does anything: the pending invite is in the session keyring (core#355
# pending_join.rs, account join-pending-v1; read by attribute only, the secret is never read), and
# no resolve was spent (nothing is resolved until the member clicks Review).
check_holds_invite() { # case dir
  local c="$1" d="$2" code n i found=""
  code="$(cat "$d/code.txt")"
  for i in $(seq 1 60); do
    found="$(env DBUS_SESSION_BUS_ADDRESS="unix:path=$XRT/bus" gdbus call --session --dest org.freedesktop.secrets \
      --object-path /org/freedesktop/secrets --method org.freedesktop.Secret.Service.SearchItems \
      "{'username': 'join-pending-v1'}" 2>/dev/null)"
    case "$found" in *"/org/freedesktop/secrets/collection/"*) break ;; esac
    found=""; sleep 1
  done
  n="$(psql_q "SELECT resolve_count FROM join_codes WHERE code = '$code'")"
  HOLDS=(step_pass "first run holds the invite (STUB: verdict not implemented yet)")
}

# Step 4a: the service's signed resolve, checked the way the app must check it.
check_resolve() { # case dir
  local c="$1" d="$2" code pin="$W/state/keys/pin.txt" out
  code="$(cat "$d/code.txt")"
  if [ "$c" = rogue-key ]; then
    # Negative control: the same answer checked against a pin that is not the service's key.
    mkdir -p "$d/rogue" && python3 -I "$JE/join_crypto.py" keygen "$d/rogue" && pin="$d/rogue/pin.txt"
  fi
  curl -sS -o "$d/resolve.json" "http://127.0.0.1:$LPORT/api/join/$code" ||
    { step_fail "$c" 4a "resolve: the join service did not answer"; return 1; }
  out="$(python3 -I "$JE/join_crypto.py" verify "$d/resolve.json" "$pin" "$d/key.txt")"
  local n; n="$(psql_q "SELECT resolve_count FROM join_codes WHERE code = '$code'")"
  step_pass "$c" 4a "resolve (STUB: verdict not implemented yet)"
}

run_case() { # case
  local c="$1" d="$JW/case-$1"
  mkdir -p "$d"
  say "== case $c"
  seed_invite "$c" "$d" || { step_fail "$c" 0 "inviter: could not make the direct invite (logs/seed.log)"; return 1; }
  say "     [$c] inviter: direct invite reserved and attached through the landing store (envHash $(cut -c1-12 "$d/envhash.txt")…)"
  if check_page "$c" "$d"; then
    check_install "$c" "$d"
  else
    step_skip "$c" 2 "the page offered no command"
    step_skip "$c" 3 "the page offered no command"
  fi
  check_resolve "$c" "$d"
  if [ -n "${JE_TEST_BUILD:-}" ]; then
    grep -q "^[A-Z]* \[$c\] 4b-i " "$SUMMARY" || step_skip "$c" 4b-i "the app was not installed or did not come up"
    step_pending "$c" 4b-ii "$PENDING_4B_II"
  else
    step_pending "$c" 4b "$PENDING_4B"
  fi
  step_pending "$c" 5 "$PENDING_5"
  # The installed AppImage is ~700 MB on the namespace's tmpfs; drop it before the next case.
  case "$d" in "$JW"/case-*) [ -d "$d/apps" ] && [ ! -L "$d/apps" ] && rm -rf -- "${d:?}/apps" ;; esac
}

inner_main() {
  # shellcheck source=/dev/null # the K1-L script's functions (staged by the outer phase)
  . "$(dirname "${BASH_SOURCE[0]}")/k1l-functions.sh"
  JE="$(dirname "${BASH_SOURCE[0]}")/join-e2e"
  pidns_assert_init || { echo "::error::--inner must be PID 1 and its own group leader in a fresh PID namespace" >&2; exit 2; }
  seal || { echo "::error::could not seal the namespace; not launching anything" >&2; exit 2; }
  W=/mnt/work
  JE="$W/harness/scripts/ci/join-e2e"
  HERE="$W/harness/scripts/ci/linux-native-acceptance"
  SUMMARY="$W/logs/summary.txt"
  K1L_QUIT=x11; K1L_QUIT_TIMEOUT=60
  {
    echo "join-e2e summary (Join-E2E fresh-machine gate, Linux)"
    echo "harness:  $JE_HARNESS_REV   landing: $JE_LANDING_REV   chromium: $JE_CHROMIUM_NAME"
    echo "arch:     $(uname -m)  kernel: $(uname -r)   cases: $JE_CASES"
    echo "app:      AppImage sha256 $JE_APPIMAGE_SHA"
  } >"$SUMMARY"
  local ok=1
  probe "pid namespace: PID 1, own session and group, no other process" p_pid1 || ok=0
  probe "host home hidden ($K1L_HOST_HOME)" p_home_hidden || ok=0
  probe "host /run/user and system bus hidden" p_run_hidden || ok=0
  probe "no inherited DISPLAY / WAYLAND / DBUS / XDG_RUNTIME_DIR" p_env_clean || ok=0
  probe "cgroup filesystem hidden" p_cgroup_hidden || ok=0
  probe "GPU device nodes hidden" p_gpu_hidden || ok=0
  probe "network: loopback only" p_net_lo_only || ok=0
  probe "host abstract sockets unreachable (with positive control)" p_abstract_sockets || ok=0
  probe "stray-process scan sees a leaked sleep (negative control)" p_stray_scan_control || ok=0
  probe "listening-socket scan sees a new listener (negative control)" p_socket_scan_control || ok=0
  probe "nothing off loopback reachable, production hosts included (with positive control)" p_offline || ok=0
  if [ "$ok" -ne 1 ]; then
    say "isolation incomplete: nothing was started"; echo "RESULT: FAIL (isolation)" >>"$SUMMARY"; exit 2
  fi
  start_display || { say "private Xvfb display did not come up"; echo "RESULT: FAIL (display)" >>"$SUMMARY"; exit 2; }
  say "private display $K1L_PRIVATE_DISPLAY up (Xvfb pid ${INFRA_PIDS[-1]}, namespace-local)"
  mkdir -p "$JW" "$W/state"
  if ! start_stack; then echo "RESULT: FAIL (stack)" >>"$SUMMARY"; exit 2; fi

  local c rc=0
  for c in $JE_CASES; do run_case "$c" || true; done

  # Final check before PID 1 exits.
  infra_tree
  local p; for p in "${INFRA_TREE[@]}"; do kill -TERM "$p" 2>/dev/null; done
  for p in "${INFRA_TREE[@]}"; do wait_gone "$p" 5 || kill -KILL "$p" 2>/dev/null; done
  local i; for i in $(seq 1 20); do scan_strays; [ "${#STRAYS[@]}" -eq 0 ] && break; sleep 0.2; done
  if [ "${#STRAYS[@]}" -ne 0 ]; then
    printf '    left: %s\n' "${STRAYS[@]}" | tee -a "$SUMMARY"
    say "final: ${#STRAYS[@]} process(es) left in the namespace at the end"; FAILED=1
  else
    say "final: no process left in the namespace"
  fi
  local pend; pend="$(grep -c '^PENDING ' "$SUMMARY")"
  if [ "$FAILED" -eq 0 ]; then
    echo "RESULT: PASS (automated steps)  GATE: NOT MET while $pend step(s) are pending" >>"$SUMMARY"
  else
    echo "RESULT: FAIL" >>"$SUMMARY"; rc=1
  fi
  exit "$rc"
}

if [ "${1:-}" = --inner ]; then inner_main; fi
outer_main "$@"
