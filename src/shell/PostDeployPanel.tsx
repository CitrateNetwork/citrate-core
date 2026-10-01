// =====================================================================
// citrate-core — After deploy (HUP-S6.6, US-6.1 tail)
//
// Once a hello-mint contract creation is confirmed on chain 40204: find the contract from the
// deploy receipt, verify its source on CitrateScan, switch the page to 40204, pin the built page
// to IPFS, and write a Vercel-ready export. Every step is a core command (src-tauri postdeploy.rs)
// and shows exactly what core answered, failures included. Nothing here signs or deploys to
// Vercel; the member runs the printed commands with their own Vercel login.
// =====================================================================
import { useState, type CSSProperties } from "react";
import type { ContractsDomain, VerifyOutcomeView } from "../bridge/domains";

export type PostDeployOps = Pick<
  ContractsDomain,
  "postdeployStatus" | "postdeployReceipt" | "postdeployVerify" | "postdeploySwitchSite" | "postdeployPinSite" | "postdeployVercelExport"
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
      return { ok: true, text: `${st.contractName} is at ${r.contractAddress} (block ${r.blockNumber}).` };
    }, setFound);

  const ready = !!address && !!dir && !busy;

  return (
    <div className="surface" data-testid="post-deploy" style={{ padding: 18, display: "flex", flexDirection: "column", gap: 8 }}>
      <span style={{ fontSize: 13.5, fontWeight: 500 }}>After deploy</span>
      <span style={note}>For a hello-mint project: verify the contract, point the page at chain 40204, pin it to IPFS, and export it for Vercel.</span>
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
              return { ok: true, text: `The page now targets ${r.address} on chain 40204 (${r.envPath}). Rebuild it with npm run build before pinning.` };
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
    </div>
  );
}
