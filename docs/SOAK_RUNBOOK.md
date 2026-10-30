---
created: 2026-10-26
branch: rc52/soak-monitor
author: Larry Klosowski (MAC lane; Claude Opus 5.5)
status: active (proposal: the owner and the DGX lead set the criteria numbers and the duration)
gate: RC52 G9 (soak)
tool: scripts/soak/soak-monitor.py
criteria: scripts/soak/soak-criteria.json
updated: 2026-10-26 (DGX lane: r1006 producers, profile and RPC access)
---

# 0.5.2 RC soak runbook (gate G9)

The soak runs the 0.5.2 release candidate on the rerolled testnet for a fixed time before
genesis, and records a PASS or FAIL with reasons. It sits in **Rehearsal 2** of the release
checklist: citrate-federation `.agentile/planset/2026-10-19-betanet/RELEASE_052_CHECKLIST.md`
§3. The checklist owns the order of operations and who has to be present. This runbook covers
only the soak. Read the checklist first; this page does not copy it.

The monitor is read-only. It sends JSON-RPC reads and runs `ps` and `lsof`. It never signs or
sends a transaction, never signals the app, and never starts anything.

## 1. Prerequisites

- **Rerolled chain.** The new genesis is live under the ceremony runbook (citrate-chain
  `scripts/genesis/CEREMONY_RUNBOOK.md` §6), with **≥ 4 producers on separate hosts**. For the
  RC52 soak that chain is the **r1006** testnet: seven producers, `citrate-prod-1` to
  `citrate-prod-7`, on separate hosts in seven regions, all run by one operator (Citrate Inc.).
  r1006 is prepared, not live; the soak can't start before it is up. You need:
  - the chain id (40204);
  - the new block-0 hash, from the `r1006` network profile in
    `src-tauri/network/profiles.json` once it is filled (citrate-core #518). The 12-01 genesis
    uses the `genesis-2026-12` profile instead;
  - one RPC URL per producer. The producers' RPC ports are not public: reach each producer's
    loopback RPC through an SSH tunnel from a host on the producer SSH allowlist, and name the
    tunnel's local port in `--rpc`.

  Before you start, the chain-side `fleet_agree.sh` must print `FLEET AGREE`.
- **The RC installed on test machines**, from the RC build in checklist §2:
  - one macOS machine (Apple silicon);
  - one Linux x86_64 machine;
  - one Linux aarch64 machine.

  Each app has joined and finished its first sync.

  **Never install the RC on the release Mac, and never run an install script there.** The
  release Mac only builds and signs.
- **Python 3.9 or later** on each machine. The monitor uses only the standard library.
  `lsof` counts open files on macOS; Linux reads `/proc`.
- **A checkout of this repo at the RC freeze commit.** The criteria file is versioned with the
  code, so every machine judges by the same numbers.
- **The criteria numbers are set.** `scripts/soak/soak-criteria.json` ships as a proposal (see
  section 4). The owner and the DGX lead confirm or change the numbers, and the change merges,
  before the soak starts. Never edit the numbers while a soak is running.

## 2. Start the monitor

One output directory per machine per soak attempt. The monitor refuses to append to an earlier
run. Keep the host awake for the whole soak: a sleeping host shows up as a sampling gap, and that
fails the run.

**Fleet monitor** (one, on the DGX or an ops host; chain only):

```sh
G=TBD-at-genesis(r1006.genesis_hash)   # the r1006 block-0 hash
# Keep the host awake: macOS "caffeinate -i", Linux "systemd-inhibit --what=sleep:idle".
caffeinate -i python3 scripts/soak/soak-monitor.py run \
    --rpc p1=http://127.0.0.1:<tunnel-1> --rpc p2=http://127.0.0.1:<tunnel-2> \
    --rpc p3=http://127.0.0.1:<tunnel-3> --rpc p4=http://127.0.0.1:<tunnel-4> \
    --rpc p5=http://127.0.0.1:<tunnel-5> --rpc p6=http://127.0.0.1:<tunnel-6> \
    --rpc p7=http://127.0.0.1:<tunnel-7> \
    --rpc public=https://rpc.citrate.ai \
    --chain-id 40204 --genesis-hash "$G" \
    --interval 60 --duration 72h --out ~/soak/rc52-<rc>-fleet
```

**App monitor** (one on each test machine, next to the running RC):

```sh
# Find the app's main process: the one whose children are the sidecars.
#   macOS:  pgrep -x "Citrate Core"     Linux:  pgrep -x citrate-core
# Confirm with: ps -o pid,ppid,comm -p <pid>  and  pgrep -P <pid> -l
PID=<app pid>
DATA="$HOME/Library/Application Support/ai.citrate.core"   # Linux: $HOME/.local/share/ai.citrate.core
caffeinate -i python3 scripts/soak/soak-monitor.py run \
  --rpc local=http://127.0.0.1:8545 --rpc public=https://rpc.citrate.ai \
  --chain-id 40204 --genesis-hash "$G" \
  --pid "$PID" --data-dir "$DATA" \
  --interval 60 --duration 72h --out ~/soak/rc52-<rc>-<macos|linux-x64|linux-arm64>
```

`local` is the app's own node, and `public` is the fleet. The fork check compares them at every
sample.

Run each monitor under `tmux` or `nohup` so a closed terminal does not stop it. Ctrl-C or
SIGTERM stops a run cleanly and still writes `verdict.json`, which records `"interrupted": true`.

**Checking progress** at any time (the samples file is appended live):

```sh
python3 scripts/soak/soak-monitor.py verdict --samples ~/soak/<dir>/samples.jsonl
```

Before the minimum duration has passed, `duration` reads FAIL. Every other rule shows how the run
is going so far.

**Do not touch the app during the soak.** Don't open settings, toggle services, update or sleep
the machine. If someone has to restart a sidecar, write it down: section 5 explains how.

## 3. Duration

- **Minimum: 24 hours** (`run.min_duration_hours: 24`). A shorter run cannot PASS.
- **Preferred: 72 hours.** Start the monitors with `--duration 72h`. For RC52 the owner set
  72 hours (federation RC52 planset, decision D-3, 2026-10-26); `min_duration_hours` in the
  criteria file still reads the proposed 24 until the criteria numbers are set.

  A 24-hour run sees one daily cycle. A 72-hour run also catches slow leaks (RSS, file
  descriptors, DB growth) and producer restarts that only happen every day or two.
- The clock starts when the **last** monitor starts. Every machine must cover the same window.
- If any machine aborts (section 6), the soak restarts from zero on every machine.

## 4. Pass criteria (what the verdict checks)

The numbers live in `scripts/soak/soak-criteria.json`. Each rule reports `ok`, `FAIL` or `skip`.
The verdict is **PASS only when no rule FAILs**. Missing evidence fails closed: an RPC that never
reports a peer count fails `peers`, and too little data after warm-up fails `rss_growth`.

| Rule | Checks | Proposed default | Why this number |
|---|---|---|---|
| `duration` | first to last sample | ≥ 24 h | See section 3 |
| `sampling_gaps` | longest gap between samples | ≤ 5 min | Catches a host that slept or a monitor that hung |
| `chain_identity` | every answer's chain id and block-0 hash match `--chain-id` / `--genesis-hash` | exact | A wrong genesis is the r1005-era failure mode |
| `rpc_reachable` | an endpoint unreachable N samples in a row | ≤ 2 | Allows for a network blip, but not an outage |
| `rpcs_agree` | same block-0 hash across endpoints; head spread | ≤ 120 blocks | Same spread as `fleet_agree.sh` `MAX_SPREAD` |
| `no_fork` | block hash at (lowest head − 6) equal on every endpoint; each endpoint re-reads its previous check height and finds it unchanged. A failed re-read is never read as unchanged: the height is kept and re-read at the next sample, and a height still unverified at the end of the run FAILs | none allowed | `fleet_agree.sh` method. The re-read catches a reorg that reaches the previous check height, which is about lag + one interval of blocks deep (about 36 at r1005's ~30 blocks/min with lag 6 and 60 s sampling). The re-read does not see a shallower reorg; the cross-endpoint comparison catches one only while the endpoints disagree |
| `head_advances` | per endpoint, blocks gained in every 10-min window | ≥ 100 | r1005 made ~300 per 10 min in the self-check (section 8). 100 leaves room for load and still catches a stall |
| `finality` | the finalized height rises at least every N min; never goes back; never above head | stall ≤ 10 min | Mode `auto`: checked only once an endpoint reports a finalized height above 0. r1005 reports 0. **If the new genesis turns finality on, set the mode to `required`** |
| `peers` | `net_peerCount` ≥ min; below the minimum for at most N min | ≥ 3 for ≤ 10 min | 4 producers means each sees at least 3 peers (r1006 has 7) |
| `app_alive` | the app PID is running at every sample | always | |
| `rss_growth` | least-squares RSS slope after a 60-min warm-up, as a % of mean RSS per hour | ≤ 1.0 %/h, with ≥ 60 min of data after warm-up | Over 72 h, 1 %/h is roughly a 70 % rise: the most a soak should allow |
| `fd_growth` | the same for open file descriptors | ≤ 2.0 %/h | Catches socket and file-handle leaks |
| `sidecar_restarts` | sidecar restarts from supervisor crash records written since the start, or child processes replaced or vanished (counted per name; the larger wins), minus the explained ones. The expected sidecars are the app's children at the first sample after warm-up; each later disappearance of one counts as a restart, even with no crash record | 0 unexplained | Every restart is either a bug or has a written reason. A sidecar that stops with no crash record (a stopped supervisor monitor) must not pass silently |

A chain-only monitor (no `--pid`) reports the four app rules as `skip`. The fleet's PASS means
the chain rules passed. Each app machine's PASS adds the app rules.

`scripts/soak/soak-monitor.test.mjs` proves every key moves the verdict. If you add a key to the
criteria without a rule that reads it, the tests fail.

## 5. Explaining a restart

When a sidecar restart has a known reason that is not an RC defect (for example, an operator
restarted a producer and the app's node reconnected), write it down in `explanations.json`, next
to the samples:

```json
{"restarts": [{"sidecar": "citrate-node", "count": 1, "reason": "p2 restarted 14:05Z for a disk swap (ops log link)"}]}
```

Then recompute the verdict:

```sh
python3 scripts/soak/soak-monitor.py verdict --samples <dir>/samples.jsonl \
  --explanations <dir>/explanations.json --out <dir>/verdict.json
```

Each entry needs a sidecar name, a count and a reason. The DGX lead or the owner signs off the
reasons in the evidence. A restart storm cannot be explained away: abort (section 6).

## 6. Abort criteria (stop early, the soak FAILs)

Stop every monitor, record the verdict as it stands, and restart the soak from zero on a fixed
RC if any of these happens:

- **Fork or wrong chain.** `no_fork`, `chain_identity` or `rpcs_agree` (different genesis) fails
  on any sample. Page the DGX lead; this is a chain incident before it is a soak result.
- **Head stall.** No endpoint gains a block for 30 min.
- **Finality stall.** When finality is on, the finalized height stalls for 30 min.
- **App exit.** The app exits or is killed on any test machine (`app_alive`).
- **Restart storm.** 3 or more restarts of one sidecar within an hour.
- **Runaway memory.** The app's RSS doubles from its post-warm-up level.
- **Wrong build or a touched machine.** The wrong build turns out to be installed, a test
  machine slept, or someone used the app during the window.

Each abort becomes an item on the Project #5 board, with the evidence directory attached.

## 7. Evidence and recording the result

From each machine, collect:

- `samples.jsonl` and `verdict.json` (and `explanations.json`, if any).
- Machine details:
  - OS and architecture (the samples header records them);
  - the RC version and build checksum;
  - the freeze commit of this repo the monitor ran from.
- At the end, on Linux: the K1-L `final` check output (`docs/LIFECYCLE_FAULTS_LINUX.md`). On
  macOS: quit the app, then `pgrep -P <old pid>` and `pgrep -fl citrate` show nothing left
  running.
- A screenshot of the `node/monitoring` Grafana testnet dashboard over the soak window. The
  alerts should have stayed quiet.

Where the evidence goes:

1. File the evidence under the rehearsal-2 EVIDENCE of the betanet planset (federation repo,
   private). The samples hold RPC host names, heights, hashes and process names. They hold no
   keys, tokens or paths. Keep URL tokens out of the `--rpc` labels: the default label is the
   host only.
2. Post a comment on the G9 item of **Project #5**:
   - `G9 soak <PASS|FAIL>`;
   - the window (UTC start to end);
   - the four or more verdict lines (fleet, macOS, Linux x86_64, Linux aarch64);
   - the evidence link.

   The lane lead updates the item's Status and links (full URLs). Agents do not edit the board.
3. A FAIL lists every FAIL reason. The owner makes the go/no-go call in checklist §3.

## 8. Self-check against r1005 (2026-10-26)

The tool was proven read-only against the live r1005 chain before any reroll:

- 5 minutes;
- interval 30 s;
- one endpoint, `https://rpc.citrate.ai`;
- chain 40204, genesis `0x1dcfc490…75f1`;
- no app.

The trimmed output is in `scripts/soak/examples/selfcheck-r1005/`. The chain rules passed:

- identity;
- reachability;
- agreement;
- fork check with re-read;
- peers = 4;
- finality `ok`, not active (r1005 reports a finalized height of 0).

`duration` and `head_advances` FAIL by design: 5 minutes is shorter than both the 24-hour minimum
and one 10-minute window. Unknown evidence fails closed. The head gained about 150 blocks in 5 minutes
(about 30 a minute).
