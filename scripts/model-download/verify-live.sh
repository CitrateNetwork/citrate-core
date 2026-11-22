#!/bin/bash
# Scripted verification of the onboarding local-model download, on a Linux build host (root, for
# the low-disk tmpfs). It runs the REAL download path (`model::live_tests::live_download_and_verify`,
# the code `model_download` runs) against:
#   a   the real chain (citrate.ai -> Hugging Face -> signed Xet CDN), full 4.59 GB + SHA-256
#   r*  the real chain through a throttling proxy: 2 Mbit/s with a mid-download kill and restart,
#       a 4-minute outage, a stall
#   f*  a local fault server (faultsrv.py) that expires signed URLs, drops, stalls, goes down,
#       serves wrong bytes
#   e   a 1 GB tmpfs (low disk)
#
# This is a manual, scripted check, not a CI job: the real-chain runs pull several GB and take
# about 30 minutes in total. CI covers the same failure modes with the loopback tests in
# src-tauri/src/model_dl_tests.rs.
#
# Usage: verify-live.sh <test-binary> <work-dir> [real]
#   <test-binary>  target/debug/deps/citrate_core_lib-<hash> (cargo test -p citrate-core --lib --no-run)
#   <work-dir>     an empty dir with about 12 GB free; removed parts are left for inspection
#   real           also run the real-network scenarios (a, r*)
# Each scenario writes <work-dir>/<name>.log; grep "LIVE RESULT" for the outcome.
# Never run this on a Mac or against a user's app data.
set -u
B=$(readlink -f "$1"); O=$(readlink -f "$2"); REAL=${3:-}
S=$(cd "$(dirname "$0")" && pwd)
mkdir -p "$O"; cd "$O"
T="model::live_tests::live_download_and_verify --exact --ignored --nocapture"
mark() { echo "$(date +%s.%N | cut -c1-12) === $*" | tee -a "$O/marks.log"; }
run() { # name urls [size sha] -> starts the harness in the background, HP = pid
  mkdir -p "$O/$1"
  MDL_DIR="$O/$1" MDL_URLS="$2" MDL_SIZE="${3:-}" MDL_SHA="${4:-}" $B $T > "$O/$1.log" 2>&1 &
  HP=$!
}

if [ "$REAL" = real ]; then
  mark a-real-full; run a ""; wait $HP
  FILE="$O/a/gemma-4-E4B-it-Q4_0.gguf"
  pctl() { echo "$1" > "$O/pctl.json"; }
  pctl '{}'
  python3 "$S/throttleproxy.py" 18480 "$O/pctl.json" "$O/proxy.log" & PP=$!
  sleep 1
  prun() { mkdir -p "$O/r"; HTTPS_PROXY=http://127.0.0.1:18480 MDL_DIR="$O/r" $B $T > "$O/$1.log" 2>&1 & HP=$!; }
  mark r1-throttled-2mbit; pctl '{"rate":250000}'; prun r1; sleep 300; kill -9 $HP; ls -l "$O/r" >> "$O/marks.log"
  mark r2-restart-resumes; pctl '{"rate":4000000}'; prun r2; sleep 20
  mark r2-outage-240s; pctl '{"down":true}'; sleep 240; pctl '{"rate":4000000}'; mark r2-network-back
  sleep 30; mark r2-stall-60s; pctl '{"stall":true}'; sleep 60; pctl '{}'; mark r2-unstall
  wait $HP; kill $PP
else
  FILE=""
fi

# Fault-server scenarios over a 256 MiB prefix of the real file (or of any GGUF given as FX).
FX=${FX:-$O/fx.gguf}
if [ ! -f "$FX" ]; then
  if [ -n "$FILE" ] && [ -f "$FILE" ]; then head -c 268435456 "$FILE" > "$FX"
  else echo "FX=<a GGUF file> is required without 'real'"; exit 2; fi
fi
FS=$(stat -c %s "$FX"); FH=$(sha256sum "$FX" | cut -d' ' -f1)
ctl() { echo "$1" > "$O/ctl.json"; }
ctl '{}'; echo '{}' > "$O/ctl2.json"
python3 "$S/faultsrv.py" "$FX" 18471 "$O/ctl.json" "$O/srv.log" & P1=$!
python3 "$S/faultsrv.py" "$FX" 18472 "$O/ctl2.json" "$O/srv2.log" & P2=$!
sleep 1
U1=http://127.0.0.1:18471/model; U2=http://127.0.0.1:18472/model
mark f1-signed-urls-expire-and-are-cut; ctl '{"expiry_s":15,"rate":3000000,"cut_at_expiry":true}'; run f1 $U1 $FS $FH; wait $HP
mark f2-drops; ctl '{"drop_after":5000000}'; run f2 $U1 $FS $FH; wait $HP
mark f3-stall-once; ctl '{"stall_after":3000000}'; run f3 $U1 $FS $FH; sleep 8; ctl '{}'; wait $HP
mark f4-outage-240s-both-sources; ctl '{"rate":2000000}'; echo '{"rate":2000000}' > "$O/ctl2.json"
run f4 "$U1,$U2" $FS $FH; sleep 10; ctl '{"down":true}'; echo '{"down":true}' > "$O/ctl2.json"
sleep 240; ctl '{}'; echo '{}' > "$O/ctl2.json"; mark f4-network-back; wait $HP
mark f5-wrong-bytes-on-one-source; ctl '{"wrong_bytes":true}'; run f5 "$U1,$U2" $FS $FH; wait $HP
mark f6-slow-250KBps-kill-restart; ctl '{"rate":250000}'; run f6 $U1 $FS $FH; sleep 240; kill -9 $HP
ctl '{}'; mv "$O/f6.log" "$O/f6-before-kill.log"; run f6 $U1 $FS $FH; wait $HP
mark f7-origin-serves-a-web-page; ctl '{"html":true}'; run f7 "$U1,$U2" $FS $FH; wait $HP
ctl '{}'

# Low disk: a 1 GB tmpfs and the full-size pin. The download must refuse before any byte.
if [ -n "$FILE" ] && [ "$(id -u)" = 0 ]; then
  python3 "$S/faultsrv.py" "$FILE" 18473 "$O/ctl3.json" "$O/srv3.log" & P3=$!; echo '{}' > "$O/ctl3.json"; sleep 1
  mkdir -p "$O/e" && mount -t tmpfs -o size=1G tmpfs "$O/e"
  mark e-low-disk; MDL_DIR="$O/e" MDL_URLS=http://127.0.0.1:18473/model $B $T > "$O/e.log" 2>&1
  ls -la "$O/e" >> "$O/e.log"; umount "$O/e"; kill $P3
fi
kill $P1 $P2
mark done
grep -H "LIVE RESULT" "$O"/*.log
