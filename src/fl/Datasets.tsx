// =====================================================================
// citrate-core: Groups › Rounds › Datasets (FL design board 08)
//
// Local first: nothing is shared unless a version is published. What this build has is core's
// training-set builder for verified Hermes conversations (fl_trajectories.rs: the consent switch,
// off by default, the count of assembled sets, and a build that returns path, sha256 and rows).
// That is the one dataset source that works today; the board says so for the others:
//  - Local files: not in this build.
//  - Collect from a site: not in this build. When it is, a site whose robots.txt disallows
//    collection, or that states no license, is refused.
//  - Clean with Hermes (accept or reject each change as a diff): not in this build.
//  - Publish: DatasetRegistry is not available yet on this network, and the local PII scan that
//    must pass first is not in this build, so there is no publish button.
// =====================================================================
import { useEffect, useState } from "react";
import type { FlRoundsDomain, FlTrainingSet, FlTrajectoryStatus } from "../bridge/domains";
import { ModalDialog } from "../shell/ModalDialog";
import { contractWords, NOT_AVAILABLE, type FlNetworkState } from "./networkState";
import { Card, Chip, Eyebrow, KV, Note, shortHex, Unknown } from "./primitives";
import { TrajectoryConsent } from "./TrajectoryConsent";

export type BuildSource = "files" | "traj" | "scrape";

export interface DatasetsViewProps {
  state: FlNetworkState;
  fl: FlRoundsDomain;
  toast: (m: string) => void;
  go: (route: string) => void;
}

export function DatasetsView({ state, fl, toast, go }: DatasetsViewProps) {
  const [st, setSt] = useState<FlTrajectoryStatus | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [built, setBuilt] = useState<FlTrainingSet | null>(null);
  const [pane, setPane] = useState<"overview" | "clean" | "publish">("overview");
  const [build, setBuild] = useState<BuildSource | null>(null);
  const [busy, setBusy] = useState(false);

  const refresh = () =>
    fl.trajectoryStatus().then(
      (s) => {
        setSt(s);
        setErr(s.loadError);
      },
      (e) => setErr(e instanceof Error ? e.message : String(e)),
    );
  useEffect(() => {
    void refresh();
  }, []);

  const doBuild = async () => {
    setBusy(true);
    setErr(null);
    try {
      setBuilt(await fl.buildTrainingSet(500));
      setBuild(null);
      toast("Training set built on this device.");
      await refresh();
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const count = st?.datasets ?? null;
  const registry = contractWords(state.contracts.DatasetRegistry).text;
  const consentOn = st?.settings.enabled === true;

  return (
    <div className="fl-cols" data-testid="datasets">
      <aside aria-label="Datasets" style={{ flex: "0 0 280px", borderRight: "1px solid var(--line-1)", padding: 14, display: "flex", flexDirection: "column", gap: 10 }}>
        <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
          <Eyebrow>On this device</Eyebrow>
          <span style={{ flex: 1 }} />
          <button type="button" className="btn btn-primary btn-sm" onClick={() => setBuild("traj")}>
            Build a dataset
          </button>
        </div>
        {err && (
          <p role="alert" style={{ margin: 0, fontSize: 12, color: "var(--warn)" }}>
            {err}
          </p>
        )}
        {st === null && !err ? (
          <span className="fl-muted" style={{ fontSize: 12.5 }}>
            Reading the datasets on this device.
          </span>
        ) : count === null ? (
          <span style={{ fontSize: 12.5 }}>
            Datasets: <Unknown reason="the training data store was not read" />
          </span>
        ) : count === 0 && !built ? (
          <Card testid="datasets-empty">
            <b style={{ fontWeight: 500, fontSize: 13 }}>No datasets yet</b>
            <span className="fl-muted" style={{ fontSize: 12 }}>
              Build one from verified Hermes conversations on this device.
            </span>
          </Card>
        ) : (
          <button type="button" className="fl-card" aria-current="page" style={{ padding: "10px 12px", textAlign: "left", display: "flex", flexDirection: "column", gap: 4, color: "var(--tx-1)", border: "1px solid var(--accent-deep)", cursor: "default" }}>
            <b style={{ fontWeight: 500, fontSize: 13 }}>Verified Hermes conversations</b>
            <span style={{ display: "flex", gap: 6, flexWrap: "wrap" }}>
              <Chip>License not set</Chip>
              <Chip tone="ok">Redacted</Chip>
            </span>
            <span className="fl-muted" style={{ fontSize: 12 }}>
              {count ?? 0} training set{count === 1 ? "" : "s"} assembled on this device
            </span>
          </button>
        )}
        <Note>Local first. Nothing is shared unless you publish a version.</Note>
      </aside>

      <section className="fl-main" aria-labelledby="ds-h">
        <div style={{ display: "flex", alignItems: "baseline", gap: 10, flexWrap: "wrap" }}>
          <h1 id="ds-h" className="fl-disp" style={{ fontSize: 22 }}>
            {built || (count ?? 0) > 0 ? "Verified Hermes conversations" : "Datasets"}
          </h1>
          {built && <span className="mono fl-faint" style={{ fontSize: 11.5 }}>sha256 {shortHex(built.sha256, 10, 6)}</span>}
        </div>
        <div role="tablist" aria-label="Dataset" style={{ display: "flex", gap: 4, borderBottom: "1px solid var(--line-1)" }}>
          {(
            [
              ["overview", "Overview"],
              ["clean", "Clean with Hermes"],
              ["publish", "Publish"],
            ] as const
          ).map(([id, label]) => (
            <button key={id} type="button" role="tab" aria-selected={pane === id} className="btn btn-sm btn-ghost" style={{ border: "none", borderBottom: pane === id ? "2px solid var(--accent)" : "2px solid transparent", borderRadius: 0 }} onClick={() => setPane(id)}>
              {label}
            </button>
          ))}
        </div>
        {pane === "overview" &&
          (built ? (
            <>
              <KV
                rows={[
                  { k: "Rows", v: built.examples.toLocaleString("en-US"), mono: true },
                  { k: "Files read", v: built.filesRead.toLocaleString("en-US"), mono: true },
                  { k: "Skipped lines", v: built.skippedLines.toLocaleString("en-US"), mono: true },
                  { k: "Over the cap", v: built.overCap.toLocaleString("en-US"), mono: true },
                  { k: "Privacy label", v: "Redacted, verified conversations only" },
                  { k: "License", v: "Not set" },
                  { k: "Saved at", v: <span className="mono">{built.path}</span> },
                  { k: "SHA-256", v: <span className="mono">{built.sha256}</span> },
                ]}
              />
              <Note>This build keeps one version per build. A version chain arrives with dataset versions.</Note>
              <div style={{ display: "flex", gap: 8 }}>
                <button type="button" className="btn btn-ghost" onClick={() => go("groups/rounds")}>
                  Offer to rounds
                </button>
              </div>
            </>
          ) : (count ?? 0) > 0 ? (
            <p className="fl-muted" style={{ margin: 0, fontSize: 13 }}>
              {count} training set{count === 1 ? " is" : "s are"} assembled on this device. Build again to see a set's rows and hash here.
            </p>
          ) : (
            <p className="fl-muted" style={{ margin: 0, fontSize: 13 }}>
              No datasets yet. Build a dataset to start.
            </p>
          ))}
        {pane === "clean" && (
          <Card testid="clean-unavailable">
            <b style={{ fontWeight: 500, fontSize: 13 }}>Cleaning with Hermes is not in this build</b>
            <span className="fl-muted" style={{ fontSize: 12.5 }}>
              When it is, Hermes proposes each change as a diff you accept or reject, and accepting saves a new version while keeping the old one.
            </span>
          </Card>
        )}
        {pane === "publish" && (
          <Card testid="publish-unavailable">
            <b style={{ fontWeight: 500, fontSize: 13 }}>DatasetRegistry: {registry}</b>
            <span className="fl-muted" style={{ fontSize: 12.5 }}>
              Publishing uploads a version with its license and privacy label and records it on DatasetRegistry. The local PII scan that must pass first is not in this build, so there is no publish button. You can't unpublish a CID once others pin it.
            </span>
          </Card>
        )}
      </section>

      {build && (
        <ModalDialog
          labelledBy="ds-build-h"
          focusKey={"build-" + build}
          register="charter"
          zIndex={55}
          onEscape={() => setBuild(null)}
          panelStyle={{ width: "min(560px, 100%)", maxHeight: "calc(100vh - 48px)", overflow: "auto", background: "var(--srf-2)", color: "var(--tx-1)", border: "1px solid var(--line-2)", borderRadius: 8, display: "flex", flexDirection: "column" }}
        >
          <div style={{ display: "flex", alignItems: "center", padding: "12px 18px", borderBottom: "1px solid var(--line-1)" }}>
            <h2 id="ds-build-h" style={{ margin: 0, fontSize: 15, fontWeight: 600 }}>
              Build a dataset
            </h2>
            <span style={{ flex: 1 }} />
            <button type="button" className="btn btn-ghost btn-sm" data-autofocus onClick={() => setBuild(null)}>
              Close
            </button>
          </div>
          <div style={{ padding: "14px 18px", display: "flex", flexDirection: "column", gap: 12 }}>
            <div role="radiogroup" aria-label="Source" style={{ display: "flex", gap: 4, flexWrap: "wrap" }}>
              {(
                [
                  ["files", "Local files"],
                  ["traj", "Hermes conversations"],
                  ["scrape", "Collect from a site"],
                ] as const
              ).map(([id, label]) => (
                <button key={id} type="button" role="radio" aria-checked={build === id} className={build === id ? "btn btn-sm btn-secondary" : "btn btn-sm btn-ghost"} onClick={() => setBuild(id)}>
                  {label}
                </button>
              ))}
            </div>
            {build === "files" && <Note>Building from a folder on this device is not in this build. When it is, files are read in place and nothing leaves this device.</Note>}
            {build === "scrape" && (
              <div role="alert" className="fl-card" style={{ padding: "10px 12px", borderColor: "var(--danger)", background: "var(--danger-bg)", fontSize: 12.5 }}>
                <b style={{ fontWeight: 600 }}>Collecting from a site is not in this build.</b> When it is, a site whose robots.txt disallows collection, or that states no license, is refused, and Hermes will not collect it.
              </div>
            )}
            {build === "traj" && (
              <>
                <span style={{ fontSize: 13 }}>Use verified Hermes conversations from this device. Training on them needs your consent, which is off by default.</span>
                <TrajectoryConsent fl={fl} toast={toast} onStatus={setSt} onBuilt={(b) => { setBuilt(b); setBuild(null); }} />
              </>
            )}
          </div>
          <div style={{ display: "flex", gap: 8, justifyContent: "flex-end", padding: "12px 18px", borderTop: "1px solid var(--line-1)" }}>
            <button type="button" className="btn btn-ghost" onClick={() => setBuild(null)}>
              Cancel
            </button>
            <button type="button" className="btn btn-primary" data-testid="ds-build" disabled={build !== "traj" || !consentOn || busy} onClick={() => void doBuild()}>
              Build locally
            </button>
          </div>
        </ModalDialog>
      )}
    </div>
  );
}

export const DATASET_REGISTRY_WORDS = NOT_AVAILABLE;
