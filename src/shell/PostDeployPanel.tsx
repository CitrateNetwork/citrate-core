// =====================================================================
// citrate-core — After deploy (HUP-S6.6, US-6.1 tail)
//
// Once a hello-mint contract creation is confirmed on chain 40204: find the contract from the
// deploy receipt, verify its source on CitrateScan, switch the page to 40204, pin the built page
// to IPFS, and write a Vercel-ready export. Every step is a core command (src-tauri postdeploy.rs)
// and shows exactly what core answered, failures included. Nothing here signs or deploys to
// Vercel; the member runs the printed commands with their own Vercel login.
//
// CORE-A3 (0.5.3): once the contract is found, core keeps a durable deploy record (address,
// init-code hash, compiler settings, standard-JSON input, ABI, gate record), encrypted on this
// computer per project (src-tauri deploy_records.rs). The panel says whether it was saved and
// lists the project's saved deploys.
// =====================================================================
import { useState, type CSSProperties } from "react";
import type { ContractsDomain, DeployRecordSavedView, DeployRecordSummaryView, VerifyOutcomeView } from "../bridge/domains";
import { CHAIN_ID_TEXT } from "../network/profile";

export type PostDeployOps = Pick<
  ContractsDomain,
  "postdeployStatus" | "postdeployReceipt" | "postdeployVerify" | "postdeploySwitchSite" | "postdeployPinSite" | "postdeployVercelExport" | "deployRecordSave" | "deployRecords"
>;

type Out = { ok: boolean; text: string } | null;

const note: CSSProperties = { fontSize: 11.5, lineHeight: 1.6, color: "var(--tx-2)" };
const mono: CSSProperties = { fontFamily: "var(--font-mono)", fontSize: 11 };

const msg = (e: unknown) => (e instanceof Error ? e.message : String(e));

function verifyText(v: VerifyOutcomeView): Out {
  const tail = v.message ? `: ${v.message}` : "";
  switch (v.status) {
    case "verified":
      return { ok: true, text: `Verified on CitrateScan${v.contractName ? ` as ${v.contractName}` : ""}.` };
    case "partial":
      return { ok: false, text: `Partial match only (not verified)${tail}` };
    case "failed":
      return { ok: false, text: `Not verified: CitrateScan compiled the source and it did not match${tail}` };
    default:
      return { ok: false, text: `Not verified: CitrateScan was not available${tail}` };
  }
}

/** What the panel says about a deploy-record save (CORE-A3). */
export function recordText(r: DeployRecordSavedView): Out {
  const n = `${r.count} saved for this project`;
  const sj = r.record.standardJsonBytes != null ? "" : ` The compiler input was not kept: ${r.record.standardJsonNote ?? "forge did not produce it"}.`;
  if (r.alreadySaved) return { ok: true, text: `This deploy's record was already saved (${n}).` };
  return { ok: true, text: `Deploy record saved, encrypted on this computer (${n}).${sj}` };
}

/** One line per saved deploy, newest first. */
export function recordsText(rs: DeployRecordSummaryView[]): Out {
  if (rs.length === 0) return { ok: true, text: "No deploy records are saved for this project yet." };
  return {
    ok: true,
    text: [...rs]
      .reverse()
      .map((r) => `${r.contractName} at ${r.address}, block ${r.blockNumber}, solc ${r.solcVersion}, gate ${r.gateVerdict === "READY" ? "ready" : "not ready"}${r.standardJsonBytes != null ? "" : ", no compiler input"}`)
      .join("\n"),
  };
}

function Step({ n, title, children }: { n: number; title: string; children: React.ReactNode }) {
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 6, padding: "10px 0", borderTop: "1px solid var(--line-1)" }}>
      <span style={{ fontSize: 12.5, fontWeight: 500 }}>
        {n}. {title}
      </span>
      {children}
    </div>
  );
}

function Output({ id, out }: { id: string; out: Out }) {
  if (!out) return null;
  return (
    <span data-testid={id} className="mono" style={{ ...mono, color: out.ok ? "var(--tx-1)" : "var(--danger)", whiteSpace: "pre-wrap", wordBreak: "break-all" }}>
      {out.text}
    </span>
  );
}

export function PostDeployPanel({ ops, lastDeployTx, openReader }: { ops: PostDeployOps; lastDeployTx: string | null; openReader: (address: string) => void }) {
  const [project, setProject] = useState("");
  const [tx, setTx] = useState(lastDeployTx ?? "");
  const [address, setAddress] = useState<string | null>(null);
  const [found, setFound] = useState<Out>(null);
  const [verify, setVerify] = useState<Out>(null);
  const [sw, setSw] = useState<Out>(null);
  const [pin, setPin] = useState<Out>(null);
  const [exp, setExp] = useState<Out>(null);
  const [saved, setSaved] = useState<Out>(null);
  const [records, setRecords] = useState<Out>(null);
  const [busy, setBusy] = useState(false);

  const dir = project.trim();
  const run = async (f: () => Promise<Out>, set: (o: Out) => void) => {
    setBusy(true);
    try {
      set(await f());
    } catch (e) {
      set({ ok: false, text: msg(e) });
    } finally {
      setBusy(false);
    }
  };

  const pickFolder = async () => {
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const picked = await open({ directory: true, multiple: false });
      if (typeof picked === "string") setProject(picked);
    } catch (e) {
      setFound({ ok: false, text: "The folder picker is not available here; type the project path instead. (" + msg(e) + ")" });
    }
  };

  const find = () =>
    run(async () => {
      setAddress(null);
      const st = await ops.postdeployStatus(dir);
      const r = await ops.postdeployReceipt(tx.trim());
      if (!r) return { ok: false, text: "The deploy is not confirmed yet. Try again in a few seconds." };
      if (r.status === 0) return { ok: false, text: `The deploy reverted in block ${r.blockNumber}; nothing was deployed.` };
      if (!r.contractAddress) return { ok: false, text: "That transaction did not create a contract." };
      setAddress(r.contractAddress);
      // CORE-A3: keep the deploy record. A failure is shown as core gave it; finding still worked.
      try {
        setSaved(recordText(await ops.deployRecordSave(dir, tx.trim())));
      } catch (e) {
        setSaved({ ok: false, text: `The deploy record was not saved: ${msg(e)}` });
      }
      return { ok: true, text: `${st.contractName} is at ${r.contractAddress} (block ${r.blockNumber}).` };
    }, setFound);

  const ready = !!address && !!dir && !busy;

  return (
    <div className="surface" data-testid="post-deploy" style={{ padding: 18, display: "flex", flexDirection: "column", gap: 8 }}>
      <span style={{ fontSize: 13.5, fontWeight: 500 }}>After deploy</span>
      <span style={note}>For a hello-mint project: verify the contract, point the page at chain {CHAIN_ID_TEXT}, pin it to IPFS, and export it for Vercel.</span>
      <div style={{ display: "flex", gap: 6 }}>
        <input className="input" data-testid="pd-project" style={{ ...mono, flex: 1 }} placeholder="hello-mint project folder" value={project} onChange={(e) => setProject(e.target.value)} />
        <button className="btn btn-sm btn-ghost" onClick={() => void pickFolder()}>
          Choose
        </button>
      </div>
      <input className="input" data-testid="pd-tx" style={mono} placeholder="Deploy transaction hash (0x...)" value={tx} onChange={(e) => setTx(e.target.value)} />

      <Step n={1} title="Find the contract">
        <div style={{ display: "flex", gap: 6 }}>
          <button className="btn btn-sm btn-secondary" data-testid="pd-find" disabled={!dir || !tx.trim() || busy} onClick={() => void find()}>
            Find
          </button>
          {address && (
            <button className="btn btn-sm btn-ghost" data-testid="pd-open-reader" onClick={() => openReader(address)}>
              Open in Contract reader
            </button>
          )}
        </div>
        <Output id="pd-address" out={found} />
        <Output id="pd-record-out" out={saved} />
      </Step>

      <Step n={2} title="Verify on CitrateScan">
        <button className="btn btn-sm btn-secondary" data-testid="pd-verify" disabled={!ready} onClick={() => void run(async () => verifyText(await ops.postdeployVerify(dir, address ?? "", undefined)), setVerify)}>
          Verify
        </button>
        <Output id="pd-verify-out" out={verify} />
      </Step>

      <Step n={3} title="Switch the site to chain 40204">
        <button
          className="btn btn-sm btn-secondary"
          data-testid="pd-switch"
          disabled={!ready}
          onClick={() =>
            void run(async () => {
              const r = await ops.postdeploySwitchSite(dir, address ?? "");
              return { ok: true, text: `The page now targets ${r.address} on chain 40204 (${r.envPath}). Rebuild it with npm run build before pinning.${r.decodedCalls ? " The signature request for a mint from the page now shows the call by name (mint and its quantity), read from this contract's own ABI." : ""}` };
            }, setSw)
          }
        >
          Switch
        </button>
        <Output id="pd-switch-out" out={sw} />
      </Step>

      <Step n={4} title="Pin to IPFS">
        <button
          className="btn btn-sm btn-secondary"
          data-testid="pd-pin"
          disabled={!ready}
          onClick={() =>
            void run(async () => {
              const p = await ops.postdeployPinSite(dir);
              return { ok: true, text: `CID ${p.cid} (${p.files} files)\nOn this node: ${p.localGatewayUrl}\nPublic gateway: ${p.publicGatewayUrl}\n${p.note}` };
            }, setPin)
          }
        >
          Pin
        </button>
        <Output id="pd-pin-out" out={pin} />
      </Step>

      <Step n={5} title="Export for Vercel">
        <button
          className="btn btn-sm btn-secondary"
          data-testid="pd-export"
          disabled={!ready}
          onClick={() =>
            void run(async () => {
              const r = await ops.postdeployVercelExport(dir);
              return { ok: true, text: `Wrote ${r.dir}. Deploy it with your own Vercel account:\n${r.commands.join("\n")}` };
            }, setExp)
          }
        >
          Export
        </button>
        <Output id="pd-export-out" out={exp} />
      </Step>

      <Step n={6} title="Saved deploys">
        <span style={note}>Records of this project's deploys, kept encrypted on this computer. The key is in your keychain and in your recovery kit.</span>
        <button className="btn btn-sm btn-secondary" data-testid="pd-records" disabled={!dir || busy} onClick={() => void run(async () => recordsText(await ops.deployRecords(dir)), setRecords)}>
          Show
        </button>
        <Output id="pd-records-out" out={records} />
      </Step>
    </div>
  );
}
