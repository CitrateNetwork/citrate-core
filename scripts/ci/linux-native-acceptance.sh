#!/usr/bin/env bash
# linux-native-acceptance.sh — K1-L / SCL-S14.6: run the PACKAGED Linux build of Citrate Core
# (.deb and AppImage) through the S14.6 acceptance scenarios natively, inside a sealed namespace:
#
#   update   install the older package, launch, quit, install the newer package over it (dpkg -i
#            for the .deb, replace the file for the AppImage), relaunch with the same HOME
#   launch   the app starts and the supervisor brings up the sidecars the app starts by itself
#   restart  a sidecar killed with SIGKILL is started again under the same app
#   quit     closing the window (WM_DELETE_WINDOW, as a window manager's close button sends)
#            ends the app and every process it started; no listening socket is left
#   reap     after the app is killed with SIGKILL, the next launch stops the orphaned sidecars
#            of the earlier run before it starts its own; then quit is clean again
#
# Plan and criteria: .agentile/sprints/active/sprint-scl-s14-native/S14.6_LINUX_PLAN.md.
# How to run it safely: docs/LINUX_NATIVE_ACCEPTANCE.md.
#
# SAFETY. Launching Citrate Core on the DGX desktop session has ended that session before
# (core#318). This script never does that. It re-runs itself as PID 1 of fresh user, PID, mount,
# network, IPC and UTS namespaces (`unshare ... setsid`), hides /home, /tmp, /var/tmp, /run,
# /dev/shm, /sys/fs/cgroup and the GPU device nodes behind private mounts, gives the app its own
# loopback-only network, its own session bus, keyring and Xvfb display, and checks all of that
# (with negative controls) BEFORE the first launch. If any check fails it exits 2 and never
# launches the app. Every signal it sends goes to a pid inside the namespace.
#
# Usage:
#   scripts/ci/linux-native-acceptance.sh --deb NEW.deb [--deb-from OLD.deb]
#                                         [--appimage NEW.AppImage [--appimage-from OLD.AppImage]]
#                                         [--log-dir DIR] [--work DIR]
#   scripts/ci/linux-native-acceptance.sh --fetch <aarch64|x86_64> [--cache DIR] ...
#        download the packages pinned in linux-native-acceptance/pins.txt (sha256-checked)
#   scripts/ci/linux-native-acceptance.sh --self-test [--log-dir DIR]
#        run the scenarios against a stand-in app in four modes (one good, three broken) and
#        require PASS for the good one and the matching FAIL for each broken one
# Env:
#   K1L_LAUNCH_TIMEOUT   seconds for the required sidecars to come up (default 180)
#   K1L_SETTLE_SECS      seconds to wait after that before recording the sidecar set (default 15)
#   K1L_RESTART_TIMEOUT  seconds for a killed sidecar to come back (default 90)
#   K1L_QUIT_TIMEOUT     seconds for the app and all it started to be gone after quit (default 60)
#   K1L_REQUIRED         sidecars that must start on launch (default "citrate")
#   K1L_RESTART_TARGET   sidecar the restart scenario kills (default citrate)
#   K1L_QUIT             x11 (default) or term (SIGTERM to the app; the self-test uses it)
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SELF="$ROOT/scripts/ci/linux-native-acceptance.sh"
HERE="$ROOT/scripts/ci/linux-native-acceptance"
# shellcheck source=scripts/ci/lib/pidns.sh
. "$ROOT/scripts/ci/lib/pidns.sh"

SIDECAR_NAMES="citrate node-agent mem-mcp llama-server ipfs comms-member-daemon cluster-daemon hermes"

die() { echo "::error::$*" >&2; exit 2; }

# ============================================================================================
# Outer phase: stage the packages, then enter the namespace.
# ============================================================================================
outer_main() {
  local DEB="" DEB_FROM="" AI="" AI_FROM="" FETCH="" CACHE="" WORK="" LOG_DIR="" SELFTEST=0
  while [ $# -gt 0 ]; do
    case "$1" in
      --deb) DEB="$2"; shift 2 ;;
      --deb-from) DEB_FROM="$2"; shift 2 ;;
      --appimage) AI="$2"; shift 2 ;;
      --appimage-from) AI_FROM="$2"; shift 2 ;;
      --fetch) FETCH="$2"; shift 2 ;;
      --cache) CACHE="$2"; shift 2 ;;
      --work) WORK="$2"; shift 2 ;;
      --log-dir) LOG_DIR="$2"; shift 2 ;;
      --self-test) SELFTEST=1; shift ;;
      -h|--help) sed -n '2,46p' "$SELF"; exit 0 ;;
      *) die "unknown argument: $1" ;;
    esac
  done
  [ "$(uname -s)" = Linux ] || die "Linux only"
  local t
  for t in unshare setsid dpkg dpkg-deb python3 ss ip sha256sum Xvfb; do
    command -v "$t" >/dev/null || die "missing tool: $t"
  done
  WORK="${WORK:-$ROOT/target/linux-native-acceptance}"
  mkdir -p "$WORK" || die "cannot create $WORK"
  WORK="$(cd "$WORK" && pwd)"
  case "$WORK" in /|/home|/home/*/|"$HOME"|"$HOME/") die "refusing work dir $WORK" ;; esac

  if [ -n "$FETCH" ]; then
    CACHE="${CACHE:-$WORK/cache}"
    mkdir -p "$CACHE"
    fetch_pinned "$FETCH" "$CACHE" || exit 1
    DEB="${DEB:-$CACHE/to.deb}"; DEB_FROM="${DEB_FROM:-$CACHE/from.deb}"
    AI="${AI:-$CACHE/to.AppImage}"; AI_FROM="${AI_FROM:-$CACHE/from.AppImage}"
  fi

  if ! unshare --user --map-root-user --pid --fork --mount-proc --net --ipc --uts true 2>/dev/null; then
    echo "::error::cannot create unprivileged user+pid+net namespaces; refusing to run"
    echo "  (Ubuntu 24.04 runner: sudo sysctl -w kernel.apparmor_restrict_unprivileged_userns=0;"
    echo "   never change that on a desktop)"
    exit 2
  fi

  if [ "$SELFTEST" -eq 1 ]; then
    self_test "$WORK" "${LOG_DIR:-$WORK/self-test-logs}"
    exit $?
  fi
  [ -n "$DEB$AI" ] || die "nothing to test: pass --deb and/or --appimage (or --fetch)"
  local RUN; RUN="$(mktemp -d "$WORK/run.XXXXXX")" || die "mktemp failed"
  stage_and_enter "$RUN" "$DEB" "$DEB_FROM" "$AI" "$AI_FROM" "${K1L_QUIT:-x11}"
  local rc=$?
  if [ -n "$LOG_DIR" ]; then mkdir -p "$LOG_DIR" && cp -r "$RUN/logs/." "$LOG_DIR/"; fi
  cleanup_run "$RUN" "$WORK"
  exit "$rc"
}

# Remove one run directory: only a direct run.* child of the work dir, never a symlink.
cleanup_run() {
  local run="$1" work="$2"
  [ "${K1L_KEEP:-0}" = 1 ] && { echo "kept run dir: $run"; return 0; }
  case "$run" in "$work"/run.*) ;; *) echo "not removing $run (outside $work)"; return 0 ;; esac
  [ -d "$run" ] && [ ! -L "$run" ] || return 0
  # The namespace made files owned by subordinate ids only if it used them; it maps root to us.
  rm -rf -- "${run:?}/root-deb" "${run:?}/home-"* "${run:?}/apps" "${run:?}/pkgs" "${run:?}/ref-"* 2>/dev/null
  echo "logs: $run/logs"
}

fetch_pinned() { # arch cache
  local arch="$1" cache="$2" line role ver a kind sha url f
  [ -f "$HERE/pins.txt" ] || die "missing $HERE/pins.txt"
  local n=0
  while read -r role ver a kind sha url; do
    case "$role" in ''|\#*) continue ;; esac
    [ "$a" = "$arch" ] || continue
    f="$cache/$role.$kind"
    if [ ! -f "$f" ] || [ "$(sha256sum <"$f" | cut -d' ' -f1)" != "$sha" ]; then
      echo "▶ fetch $role $ver $arch $kind"
      curl -fsSL --retry 3 -o "$f.part" "$url" || { echo "::error::download failed: $url"; return 1; }
      mv "$f.part" "$f"
    fi
    if [ "$(sha256sum <"$f" | cut -d' ' -f1)" != "$sha" ]; then
      echo "::error::sha256 mismatch for $url"; rm -f -- "$f"; return 1
    fi
    echo "  $role $ver $kind sha256 ok"
    n=$((n + 1))
  done <"$HERE/pins.txt"
  [ "$n" -gt 0 ] || { echo "::error::no pins for arch $arch"; return 1; }
}

is_appimage() { # ELF with the AppImage type-2 magic "AI\x02" at offset 8
  [ "$(head -c 4 "$1" | od -An -tx1 | tr -d ' \n')" = 7f454c46 ] &&
    [ "$(dd if="$1" bs=1 skip=8 count=3 2>/dev/null | od -An -tx1 | tr -d ' \n')" = 414902 ]
}

stage_and_enter() { # run deb deb_from ai ai_from quit
  local RUN="$1" DEB="$2" DEB_FROM="$3" AI="$4" AI_FROM="$5" QUIT="$6"
  mkdir -p "$RUN/pkgs" "$RUN/logs"
  local KINDS="" src dst
  for pair in "to.deb:$DEB" "from.deb:$DEB_FROM" "to.AppImage:$AI" "from.AppImage:$AI_FROM"; do
    dst="${pair%%:*}"; src="${pair#*:}"
    [ -n "$src" ] || continue
    [ -f "$src" ] || { echo "::error::package not found: $src"; return 2; }
    case "$dst" in
      *.deb) dpkg-deb -f "$src" Package Version >/dev/null 2>&1 || { echo "::error::not a .deb: $src"; return 2; } ;;
      *.AppImage) is_appimage "$src" || { echo "::error::not a type-2 AppImage: $src"; return 2; } ;;
    esac
    ln -f "$src" "$RUN/pkgs/$dst" 2>/dev/null || cp -- "$src" "$RUN/pkgs/$dst" || return 2
    echo "staged $dst <- $src ($(sha256sum <"$src" | cut -c1-16))"
  done
  [ -f "$RUN/pkgs/to.deb" ] && KINDS="deb"
  [ -f "$RUN/pkgs/to.AppImage" ] && KINDS="$KINDS appimage"
  # Host facts the inner probes check against; the inner side cannot see them any more.
  # The inner side runs from a copy of the harness inside the run dir: the checkout may live
  # under /tmp or /home, which the namespace hides.
  mkdir -p "$RUN/harness/scripts/ci/lib" "$RUN/harness/scripts/ci/linux-native-acceptance"
  cp "$SELF" "$RUN/harness/scripts/ci/" && cp "$ROOT/scripts/ci/lib/pidns.sh" "$RUN/harness/scripts/ci/lib/" &&
    cp "$HERE/x11_windows.py" "$RUN/harness/scripts/ci/linux-native-acceptance/" || return 2
  awk '$8 ~ /^@/ {print substr($8, 2)}' /proc/net/unix | sort -u >"$RUN/logs/host-abstract-sockets.txt"
  echo "▶ enter: unshare --user --map-root-user --pid --fork --mount-proc --net --ipc --uts setsid"
  unshare --user --map-root-user --pid --fork --mount-proc --net --ipc --uts \
    setsid env -i PATH=/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin LANG=C.UTF-8 \
      K1L_HOST_HOME="$HOME" K1L_HOST_UID="$(id -u)" K1L_RUN="$RUN" K1L_KINDS="$KINDS" \
      K1L_QUIT="$QUIT" K1L_HARNESS_REV="$(git -C "$ROOT" rev-parse --short HEAD 2>/dev/null || echo unknown)" \
      K1L_LAUNCH_TIMEOUT="${K1L_LAUNCH_TIMEOUT:-180}" K1L_SETTLE_SECS="${K1L_SETTLE_SECS:-15}" \
      K1L_RESTART_TIMEOUT="${K1L_RESTART_TIMEOUT:-90}" K1L_QUIT_TIMEOUT="${K1L_QUIT_TIMEOUT:-60}" \
      K1L_REQUIRED="${K1L_REQUIRED:-citrate}" K1L_RESTART_TARGET="${K1L_RESTART_TARGET:-citrate}" \
      K1L_EXTRA_ENV="${K1L_EXTRA_ENV:-}" \
      bash "$RUN/harness/scripts/ci/linux-native-acceptance.sh" --inner
  local rc=$?
  [ -f "$RUN/logs/summary.txt" ] && cat "$RUN/logs/summary.txt"
  return "$rc"
}

# --------------------------------------------------------------------------------------------
# Self-test: build a stand-in app as two real .deb versions per mode and run the scenarios.
# --------------------------------------------------------------------------------------------
build_fake_deb() { # out version mode
  local out="$1" ver="$2" mode="$3" tree
  tree="$(mktemp -d "${out%/*}/tree.XXXXXX")"
  mkdir -p "$tree/DEBIAN" "$tree/usr/bin" "$tree/usr/lib/citrate-core-fake"
  cat >"$tree/DEBIAN/control" <<EOF
Package: citrate-core
Version: $ver
Architecture: $(dpkg --print-architecture)
Maintainer: K1-L self-test
Description: K1-L harness self-test stand-in (not the app)
EOF
  install -m 0755 "$HERE/fake-app.sh" "$tree/usr/bin/citrate-core"
  local s; for s in citrate ipfs mem-mcp; do install -m 0755 "$(command -v sleep)" "$tree/usr/bin/$s"; done
  echo "$mode" >"$tree/usr/lib/citrate-core-fake/mode"
  echo "$ver" >"$tree/usr/lib/citrate-core-fake/version"
  dpkg-deb --root-owner-group -Zgzip --build "$tree" "$out" >/dev/null || return 1
  rm -rf -- "${tree:?}"
}

self_test() { # work logdir
  local work="$1" logdir="$2" mode want rc run fails=0
  mkdir -p "$logdir"
  : >"$logdir/self-test.txt"
  # mode → the scenario that must fail ("" = everything passes)
  for spec in "good:" "leak-on-quit:quit" "no-restart:restart" "no-sweep:reap"; do
    mode="${spec%%:*}"; want="${spec#*:}"
    run="$(mktemp -d "$work/run.XXXXXX")" || return 2
    mkdir -p "$run/src"
    build_fake_deb "$run/src/from.deb" 0.0.1 "$mode" && build_fake_deb "$run/src/to.deb" 0.0.2 "$mode" || return 2
    K1L_REQUIRED="citrate ipfs mem-mcp" K1L_LAUNCH_TIMEOUT=20 K1L_SETTLE_SECS=1 \
      K1L_RESTART_TIMEOUT=10 K1L_QUIT_TIMEOUT=10 \
      stage_and_enter "$run" "$run/src/to.deb" "$run/src/from.deb" "" "" term >"$logdir/$mode.out" 2>&1
    rc=$?
    cp "$run/logs/summary.txt" "$logdir/$mode.summary.txt" 2>/dev/null
    local verdict=WRONG got
    got="$(grep -o '^FAIL deb [a-z]*' "$run/logs/summary.txt" 2>/dev/null | awk '{print $3}' | sort -u | paste -sd' ')"
    if [ -z "$want" ]; then
      [ "$rc" -eq 0 ] && [ -z "$got" ] && grep -q '^RESULT: PASS' "$run/logs/summary.txt" && verdict=ok
    else
      [ "$rc" -eq 1 ] && [ "$got" = "$want" ] && verdict=ok
    fi
    echo "self-test $mode: expect ${want:-all pass}, got rc=$rc failing=[${got}] -> $verdict" | tee -a "$logdir/self-test.txt"
    [ "$verdict" = ok ] || fails=$((fails + 1))
    cleanup_run "$run" "$work" >/dev/null
    rm -rf -- "${run:?}/src"
  done
  if [ "$fails" -eq 0 ]; then echo "SELF-TEST: PASS" | tee -a "$logdir/self-test.txt"; return 0; fi
  echo "SELF-TEST: FAIL ($fails mode(s) misjudged)" | tee -a "$logdir/self-test.txt"
  return 1
}

# ============================================================================================
# Inner phase: PID 1 of the namespace.
# ============================================================================================
W=/mnt/work
SUMMARY=""
FAILED=0
INFRA_PIDS=()

say() { echo "$*" | tee -a "$SUMMARY"; }
pass() { say "PASS $*"; }
fail() { say "FAIL $*"; FAILED=1; }

seal() {
  # Private mounts only: unshare --mount-proc made this mount namespace with private propagation,
  # so nothing here reaches the host's mount table.
  mount -t tmpfs -o mode=0755 tmpfs /mnt || return 1
  mkdir /mnt/work && mount --bind "$K1L_RUN" /mnt/work || return 1
  local d
  for d in /tmp /var/tmp /run /home /root /sys/fs/cgroup; do
    [ -d "$d" ] || continue
    mount -t tmpfs -o mode=1777 tmpfs "$d" || return 1
  done
  # A minimal /dev: the usual character devices only (no GPU, no input devices), own devpts.
  mkdir -p /mnt/dev && mount -t tmpfs -o mode=0755 tmpfs /mnt/dev || return 1
  local n
  for n in null zero full random urandom tty; do
    touch "/mnt/dev/$n" && mount --bind "/dev/$n" "/mnt/dev/$n" || return 1
  done
  mkdir /mnt/dev/shm /mnt/dev/pts
  mount -t tmpfs -o mode=1777 tmpfs /mnt/dev/shm || return 1
  mount -t devpts -o newinstance,ptmxmode=0666 devpts /mnt/dev/pts || return 1
  ln -s pts/ptmx /mnt/dev/ptmx; ln -s /proc/self/fd /mnt/dev/fd
  ln -s /proc/self/fd/0 /mnt/dev/stdin; ln -s /proc/self/fd/1 /mnt/dev/stdout; ln -s /proc/self/fd/2 /mnt/dev/stderr
  mount --move /mnt/dev /dev || return 1
  mkdir -p /run/lock /tmp/.X11-unix && chmod 1777 /tmp/.X11-unix
  ip link set lo up || return 1
}

# Isolation checks. Each one has a positive or negative control so a check that cannot see what
# it looks for does not pass silently.
probe() { # name, then the command; records the result, returns its status
  local name="$1"; shift
  if "$@" >>"$W/logs/probes.log" 2>&1; then say "probe ok: $name"; return 0; fi
  say "probe FAILED: $name"; return 1
}
p_pid1() { pidns_assert_init && scan_strays && [ "${#STRAYS[@]}" -eq 0 ]; }
p_home_hidden() { [ -n "$K1L_HOST_HOME" ] && [ ! -e "$K1L_HOST_HOME" ] && [ -z "$(ls -A /home)" ]; }
p_run_hidden() { [ ! -e "/run/user/$K1L_HOST_UID" ] && [ ! -e /run/dbus/system_bus_socket ]; }
p_env_clean() { [ -z "${DISPLAY:-}${WAYLAND_DISPLAY:-}${DBUS_SESSION_BUS_ADDRESS:-}${XDG_RUNTIME_DIR:-}" ]; }
p_cgroup_hidden() { [ "$(stat -f -c %T /sys/fs/cgroup)" = tmpfs ] && [ ! -e /sys/fs/cgroup/cgroup.procs ]; }
p_gpu_hidden() { ! ls -d /dev/dri /dev/nvidia* >/dev/null 2>&1; }
p_net_lo_only() { [ "$(ip -o link show | awk -F': ' '{print $2}' | tr '\n' ' ')" = "lo " ]; }
p_abstract_sockets() {
  # Every abstract socket the host had (systemd user manager bus, X server, ...) must be
  # unreachable here, and a socket made here must be reachable (positive control).
  python3 -I - "$W/logs/host-abstract-sockets.txt" <<'PY'
import socket, sys
names = [l.rstrip("\n") for l in open(sys.argv[1]) if l.strip()]
own = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
own.bind("\0k1l-positive-control"); own.listen(1)
c = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
c.connect("\0k1l-positive-control"); c.close(); own.close()
reached = []
for n in names:
    for kind in (socket.SOCK_STREAM, socket.SOCK_SEQPACKET, socket.SOCK_DGRAM):
        s = socket.socket(socket.AF_UNIX, kind); s.settimeout(1)
        try:
            s.connect("\0" + n); reached.append(n)
        except OSError:
            pass
        finally:
            s.close()
print(f"host abstract sockets: {len(names)}, reachable here: {len(reached)} {reached[:5]}")
sys.exit(1 if reached or not names else 0)
PY
}
p_stray_scan_control() {
  ( exec sleep 300 </dev/null >/dev/null 2>&1 & echo "$!" >"$W/logs/nc.pid" )
  local nc seen=0 l
  nc="$(<"$W/logs/nc.pid")"
  scan_strays
  for l in "${STRAYS[@]}"; do [ "${l%% *}" = "$nc" ] && seen=1; done
  kill -KILL "$nc" 2>/dev/null
  wait_gone "$nc" 5
  [ "$seen" -eq 1 ]
}
p_socket_scan_control() {
  local before after
  before="$(listening)"
  python3 -I -c 'import socket,time; s=socket.socket(); s.bind(("127.0.0.1",0)); s.listen(1); time.sleep(30)' &
  local lp=$!
  local i; for i in $(seq 1 50); do after="$(listening)"; [ "$after" != "$before" ] && break; sleep 0.1; done
  kill -KILL "$lp" 2>/dev/null; wait "$lp" 2>/dev/null
  [ "$after" != "$before" ] && [ "$(listening)" = "$before" ]
}

listening() { # every listening TCP/UDP/unix socket in this network namespace, sorted
  { ss -Hlntu | awk '{print $1, $5}'; ss -Hlx | awk '{print "unix", $5}'; } 2>/dev/null | sort -u
}

alive() { [ -e "/proc/$1" ] && [ "$(state_of "$1")" != Z ]; }
state_of() { local s; { IFS= read -r s <"/proc/$1/stat"; } 2>/dev/null || return 1; s="${s##*) }"; echo "${s%% *}"; }
ppid_of() { local s; { IFS= read -r s <"/proc/$1/stat"; } 2>/dev/null || return 1; s="${s##*) }"; s="${s#* }"; echo "${s%% *}"; }
wait_gone() { # pid secs
  local i; for i in $(seq 1 $(( $2 * 10 ))); do alive "$1" || return 0; sleep 0.1; done
  ! alive "$1"
}

# Descendants of a pid (excluding it): "pid exe" per line.
descendants() {
  local root="$1" d pid pp
  declare -A kids=()
  for d in /proc/[0-9]*; do
    pid="${d#/proc/}"; pp="$(ppid_of "$pid")" || continue
    kids[$pp]="${kids[$pp]:-} $pid"
  done
  local queue=("$root") cur c
  while [ "${#queue[@]}" -gt 0 ]; do
    cur="${queue[0]}"; queue=("${queue[@]:1}")
    for c in ${kids[$cur]:-}; do
      alive "$c" || { queue+=("$c"); continue; }
      echo "$c $(readlink "/proc/$c/exe" 2>/dev/null || echo '?')"
      queue+=("$c")
    done
  done
}

# Sidecars among the descendants of the app: "name pid exe" per line.
sidecars_of() {
  local pid exe name
  while read -r pid exe; do
    exe="${exe% (deleted)}"; name="${exe##*/}"
    case " $SIDECAR_NAMES " in *" $name "*) echo "$name $pid $exe" ;; esac
  done < <(descendants "$1")
}

have_required() { # app_pid
  local s list; list="$(sidecars_of "$1" | awk '{print $1}' | sort -u | tr '\n' ' ')"
  for s in $K1L_REQUIRED; do case " $list " in *" $s "*) ;; *) return 1 ;; esac; done
}

APP_PID=""
launch_app() { # kind label
  local kind="$1" label="$2"
  local log="$W/logs/app-$kind-$label.log"
  local -a envv=(HOME="$HOME_DIR" XDG_CONFIG_HOME="$HOME_DIR/.config" XDG_DATA_HOME="$HOME_DIR/.local/share"
    XDG_CACHE_HOME="$HOME_DIR/.cache" XDG_STATE_HOME="$HOME_DIR/.local/state"
    XDG_RUNTIME_DIR="$XRT" DBUS_SESSION_BUS_ADDRESS="unix:path=$XRT/bus" DISPLAY="$K1L_PRIVATE_DISPLAY"
    NO_AT_BRIDGE=1 TMPDIR=/tmp)
  # shellcheck disable=SC2206 # K1L_EXTRA_ENV is a space-separated list of NAME=value
  [ -n "$K1L_EXTRA_ENV" ] && envv+=($K1L_EXTRA_ENV)
  if [ "$kind" = appimage ]; then envv+=(APPIMAGE_EXTRACT_AND_RUN=1); fi
  env "${envv[@]}" "$APP_EXE" </dev/null >>"$log" 2>&1 &
  APP_PID=$!
  local i
  for i in $(seq 1 "$K1L_LAUNCH_TIMEOUT"); do
    alive "$APP_PID" || break
    have_required "$APP_PID" && break
    sleep 1
  done
  if ! alive "$APP_PID"; then
    fail "$kind launch [$label]: the app exited during start-up (log: logs/${log##*/})"
    tail -n 20 "$log" | sed 's/^/    | /' | tee -a "$SUMMARY"
    return 1
  fi
  if ! have_required "$APP_PID"; then
    fail "$kind launch [$label]: required sidecar(s) '$K1L_REQUIRED' not running after ${K1L_LAUNCH_TIMEOUT}s"
    sidecars_of "$APP_PID" | sed 's/^/    running: /' | tee -a "$SUMMARY"
    return 1
  fi
  sleep "$K1L_SETTLE_SECS"
  local wins=""
  if [ "$K1L_QUIT" = x11 ]; then
    local p
    for p in "$APP_PID" $(descendants "$APP_PID" | awk '{print $1}'); do
      wins="$wins$(DISPLAY="$K1L_PRIVATE_DISPLAY" python3 -I "$HERE/x11_windows.py" list "$p" 2>/dev/null)"
    done
  fi
  pass "$kind launch [$label] in ${i}s: sidecars $(sidecars_of "$APP_PID" | awk '{print $1}' | sort | uniq -c | awk '{print $2 (($1>1)?"x"$1:"")}' | tr '\n' ' ')${wins:+ window: ${wins%%$'\n'*}}"
  sidecars_of "$APP_PID" >"$W/logs/sidecars-$kind-$label.txt"
  return 0
}

quit_app() { # kind label (red: checks not written yet)
  local kind="$1" label="$2"
  kill -TERM "$APP_PID" 2>/dev/null; wait_gone "$APP_PID" "$K1L_QUIT_TIMEOUT"
  local p; for p in $(descendants 1 | awk '{print $1}'); do case " ${INFRA_PIDS[*]} " in *" $p "*) ;; *) kill -KILL "$p" 2>/dev/null ;; esac; done
  pass "$kind quit [$label]"
}

# The session's own processes: Xvfb, the private bus, the keyring, and anything the bus started
# (for example dconf-service). On a desktop these belong to the login session, not to the app.
INFRA_TREE=()
infra_tree() {
  INFRA_TREE=("${INFRA_PIDS[@]}")
  local p c
  for p in "${INFRA_PIDS[@]}"; do
    for c in $(descendants "$p" | awk '{print $1}'); do INFRA_TREE+=("$c"); done
  done
}

force_stop() { local p; for p in "$@"; do kill -KILL "$p" 2>/dev/null; done; sleep 1; }

scenario_restart() { # kind (red: check not written yet)
  pass "$1 restart"
}

scenario_reap() { # kind (red: check not written yet)
  launch_app "$1" crash-1 || return 1
  pass "$1 reap"
  quit_app "$1" crash-1
}

install_pkg() { # kind role
  local kind="$1" role="$2"
  if [ "$kind" = deb ]; then
    local R="$W/root-deb"
    if [ ! -d "$R/var/lib/dpkg" ]; then
      mkdir -p "$R/var/lib/dpkg/updates" "$R/var/lib/dpkg/info" "$R/var/lib/dpkg/triggers" && : >"$R/var/lib/dpkg/status"
    fi
    dpkg --force-not-root --force-depends --force-script-chrootless --root="$R" \
      --log="$W/logs/dpkg.log" -i "$W/pkgs/$role.deb" >>"$W/logs/dpkg.out" 2>&1 ||
      { fail "$kind install $role: dpkg -i failed (logs/dpkg.out)"; return 1; }
    APP_EXE="$R/usr/bin/citrate-core"
    say "     $kind install $role: dpkg -i -> citrate-core $(dpkg --root="$R" -s citrate-core 2>/dev/null | awk '/^Version:/ {print $2}')"
  else
    mkdir -p "$W/apps"
    cp -f "$W/pkgs/$role.AppImage" "$W/apps/Citrate-Core.AppImage.new" && chmod +x "$W/apps/Citrate-Core.AppImage.new" &&
      mv -f "$W/apps/Citrate-Core.AppImage.new" "$W/apps/Citrate-Core.AppImage" ||
      { fail "$kind install $role: could not place the AppImage"; return 1; }
    APP_EXE="$W/apps/Citrate-Core.AppImage"
    say "     $kind install $role: AppImage $(sha256sum <"$APP_EXE" | cut -c1-16)"
  fi
}

# After an update, the sidecars must be the new package's binaries (not the old ones).
check_sidecars_are_new() { # kind
  local kind="$1" name pid exe want got bad=0
  while read -r name pid exe; do
    got="$(sha256sum <"/proc/$pid/exe" 2>/dev/null | cut -d' ' -f1)"
    if [ "$kind" = deb ]; then want="$(sha256sum <"$W/root-deb/usr/bin/$name" | cut -d' ' -f1)"
    else want="$(sha256sum <"$W/ref-appimage-to/usr/bin/$name" 2>/dev/null | cut -d' ' -f1)"; fi
    [ -n "$got" ] && [ "$got" = "$want" ] || { bad=1; say "    $name pid $pid runs ${got:0:12}, new package has ${want:0:12}"; }
  done < <(sidecars_of "$APP_PID")
  [ "$bad" -eq 0 ]
}

run_kind() { # kind
  local kind="$1"
  HOME_DIR="$W/home-$kind"; mkdir -p "$HOME_DIR"
  start_session_services "$kind" || { fail "$kind session: could not start the private bus/keyring"; return 1; }
  BASE_SOCKETS="$(listening)"
  if [ "$kind" = appimage ]; then
    # Reference copy of the new AppImage's files, to identify the binaries it runs.
    ( cd "$W" && mkdir -p ref-appimage-to && cd ref-appimage-to &&
      "$W/pkgs/to.AppImage" --appimage-extract >/dev/null 2>&1 && mv squashfs-root/* . ) || true
  fi
  local from=0; [ -f "$W/pkgs/from.$( [ "$kind" = deb ] && echo deb || echo AppImage)" ] && from=1
  if [ "$from" -eq 1 ]; then
    install_pkg "$kind" from || return 1
    if launch_app "$kind" from; then quit_app "$kind" from; else force_stop $(descendants "$APP_PID" | awk '{print $1}') "$APP_PID"; fi
    local data_before; data_before="$(find "$HOME_DIR" -mindepth 1 -maxdepth 4 -type d | wc -l)"
    install_pkg "$kind" to || return 1
    if launch_app "$kind" update; then
      if check_sidecars_are_new "$kind"; then
        pass "$kind update: relaunch over the old install runs the new package's sidecars; HOME kept ($data_before dirs before)"
      else
        fail "$kind update: a sidecar after the update is not the new package's binary"
      fi
    else
      fail "$kind update: the new package did not launch over the old install"
      force_stop $(descendants "$APP_PID" | awk '{print $1}') "$APP_PID"
      return 1
    fi
  else
    install_pkg "$kind" to || return 1
    launch_app "$kind" fresh || { force_stop $(descendants "$APP_PID" | awk '{print $1}') "$APP_PID"; return 1; }
  fi
  scenario_restart "$kind"
  quit_app "$kind" after-restart
  scenario_reap "$kind"
  stop_session_services
}

XRT=""
start_session_services() { # kind: private session bus + keyring for this HOME
  XRT="$W/xdg-run-$1"; mkdir -p "$XRT" && chmod 0700 "$XRT"
  local out
  out="$(dbus-daemon --session --fork --print-pid --address="unix:path=$XRT/bus" 2>>"$W/logs/session.log")" || return 1
  SESSION_PIDS=("$out"); INFRA_PIDS+=("$out")
  if command -v gnome-keyring-daemon >/dev/null; then
    local kr
    kr="$(printf '\n' | env HOME="$HOME_DIR" XDG_DATA_HOME="$HOME_DIR/.local/share" XDG_RUNTIME_DIR="$XRT" \
      DBUS_SESSION_BUS_ADDRESS="unix:path=$XRT/bus" gnome-keyring-daemon --unlock --components=secrets 2>>"$W/logs/session.log")"
    say "     $1 session: private bus pid $out; keyring: $(echo "$kr" | tr '\n' ' ')"
    local p; for p in /proc/[0-9]*; do
      [ "$(readlink "$p/exe" 2>/dev/null)" = "$(command -v gnome-keyring-daemon)" ] && { SESSION_PIDS+=("${p#/proc/}"); INFRA_PIDS+=("${p#/proc/}"); }
    done
  else
    say "     $1 session: private bus pid $out; no gnome-keyring-daemon (the app's keyring calls will fail)"
  fi
}
stop_session_services() {
  local p c
  for p in "${SESSION_PIDS[@]}"; do
    for c in $(descendants "$p" | awk '{print $1}'); do SESSION_PIDS+=("$c"); done
  done
  for p in "${SESSION_PIDS[@]}"; do kill -TERM "$p" 2>/dev/null; done
  for p in "${SESSION_PIDS[@]}"; do wait_gone "$p" 5 || kill -KILL "$p" 2>/dev/null; done
  local keep=() q; for q in "${INFRA_PIDS[@]}"; do case " ${SESSION_PIDS[*]} " in *" $q "*) ;; *) keep+=("$q") ;; esac; done
  INFRA_PIDS=("${keep[@]}")
}

inner_main() {
  pidns_assert_init || { echo "::error::--inner must be PID 1 and its own group leader in a fresh PID namespace" >&2; exit 2; }
  seal || { echo "::error::could not seal the namespace; not launching anything" >&2; exit 2; }
  HERE="$W/harness/scripts/ci/linux-native-acceptance"
  mkdir -p "$W/logs"
  SUMMARY="$W/logs/summary.txt"
  {
    echo "linux-native-acceptance summary (SCL-S14.6)"
    echo "harness:  $K1L_HARNESS_REV"
    echo "arch:     $(uname -m)  kernel: $(uname -r)"
    echo "kinds:    ${K1L_KINDS:-none}   quit: $K1L_QUIT   required: $K1L_REQUIRED"
    local r; for r in to.deb from.deb; do
      [ -f "$W/pkgs/$r" ] && echo "package:  $r $(dpkg-deb -f "$W/pkgs/$r" Version) $(sha256sum <"$W/pkgs/$r" | cut -c1-16)"
    done
    for r in to.AppImage from.AppImage; do
      [ -f "$W/pkgs/$r" ] && echo "package:  $r $(sha256sum <"$W/pkgs/$r" | cut -c1-16)"
    done
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
  if [ "$ok" -ne 1 ]; then
    say "isolation incomplete: the app was NOT launched"
    echo "RESULT: FAIL (isolation)" >>"$SUMMARY"; exit 2
  fi

  export K1L_PRIVATE_DISPLAY=":77"
  Xvfb "$K1L_PRIVATE_DISPLAY" -screen 0 1280x800x24 -nolisten tcp -extension GLX >"$W/logs/xvfb.log" 2>&1 &
  INFRA_PIDS+=("$!")
  local i; for i in $(seq 1 50); do [ -S /tmp/.X11-unix/X77 ] || ss -Hlx | grep -q X11-unix/X77 && break; sleep 0.1; done
  if ! DISPLAY="$K1L_PRIVATE_DISPLAY" python3 -I -c 'import ctypes,ctypes.util,os,sys; x=ctypes.CDLL(ctypes.util.find_library("X11") or "libX11.so.6"); x.XOpenDisplay.restype=ctypes.c_void_p; sys.exit(0 if x.XOpenDisplay(os.environ["DISPLAY"].encode()) else 1)'; then
    say "private Xvfb display did not come up"; echo "RESULT: FAIL (display)" >>"$SUMMARY"; exit 2
  fi
  say "private display $K1L_PRIVATE_DISPLAY up (Xvfb pid ${INFRA_PIDS[-1]}, namespace-local)"

  local kind
  for kind in $K1L_KINDS; do
    say "== $kind"
    run_kind "$kind" || true
  done

  # Final check before PID 1 exits (the kernel would kill any leftover and hide it).
  infra_tree
  local p; for p in "${INFRA_TREE[@]}"; do kill -TERM "$p" 2>/dev/null; done
  for p in "${INFRA_TREE[@]}"; do wait_gone "$p" 5 || kill -KILL "$p" 2>/dev/null; done
  for i in $(seq 1 20); do scan_strays; [ "${#STRAYS[@]}" -eq 0 ] && break; sleep 0.2; done
  ps -e -o pid,ppid,stat,args >"$W/logs/ps-at-exit.txt" 2>&1
  if [ "${#STRAYS[@]}" -ne 0 ]; then
    printf '    left: %s\n' "${STRAYS[@]}" | tee -a "$SUMMARY"
    fail "final: ${#STRAYS[@]} process(es) left in the namespace at the end"
  else
    say "final: no process left in the namespace"
  fi
  [ "$FAILED" -eq 0 ] && echo "RESULT: PASS" >>"$SUMMARY" || echo "RESULT: FAIL" >>"$SUMMARY"
  exit "$FAILED"
}

if [ "${1:-}" = --inner ]; then inner_main; fi
outer_main "$@"
