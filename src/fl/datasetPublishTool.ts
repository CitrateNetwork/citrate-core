// =====================================================================
// citrate-core: Hermes's fl_dataset_publish_propose (0.5.4, Hermes x FL WP-H6, gap §5.4)
//
// Publishing is irreversible, so it is always HIC-1 and never inside an envelope:
//  1. core's `fl_dataset_publish_preview` names every reason it refuses (license, unknown source
//     licenses, the PII scan of exactly these rows, Public only in this build). Any refusal stops
//     here with no card;
//  2. the member decides on a card that says a CID cannot be recalled;
//  3. core's `fl_dataset_publish` writes the publish version, adds the bundle to the node's IPFS
//     and pins it (real in 0.5.4);
//  4. the DatasetRegistry record: where the address book pins DatasetRegistry and the chain has
//     code there, the exact `publish(datasetId, Version)` call opens in the wallet review for the
//     member to sign. On r1007 the book does not pin it, so the record waits and Hermes says
//     "Not available yet on this network". Hermes never signs.
//
// Kept out of harness.ts's import graph (it reads the address book JSON and uses viem), so the
// plain-Node eval tools that load the harness stay dependency-free.
// =====================================================================
import type { ApprovalCard } from "../agent/approvalCards";
import { fieldsCard } from "../agent/approvalCards";
import type { ToolAnnotation } from "../agent/toolAnnotations";
import type { CerSpec } from "../shell/state";
import { latestVersion } from "../agent/flDatasetTools";
import type { FlDatasetsDomain, FlPublication } from "./datasetTypes";
import type { FlBook } from "./networkState";
import { datasetIdFor, PUBLISH_SIGNATURE, publishCalldata, registryPrev, registryState, type RegistryReads } from "./datasetRegistry";

export interface PublishToolDeps {
  ds: Pick<FlDatasetsDomain, "list" | "publishPreview" | "publish" | "published">;
  ask: (spec: CerSpec, card: ApprovalCard) => Promise<string>;
  book: FlBook;
  reads: RegistryReads;
  /** The address that signs (the custody EOA); empty until core has read it. */
  publisher: () => string;
  /** Open the wallet review for one contract call (store.proposeContractCall). */
  propose: (input: { address: string; calldata: string; valueWei: string; label: string }) => Promise<unknown>;
}

export interface PublishOutcome {
  status: string;
  result: string;
}

const DATASET_ID = /^ds-[0-9a-f]{16}$/;
const msg = (e: unknown) => (e instanceof Error ? e.message : String(e));

/** The registry step after a publication is on IPFS. Never signs; at most opens a wallet review. */
export async function proposeRegistryRecord(name: string, p: FlPublication, deps: PublishToolDeps): Promise<string> {
  const st = await registryState(deps.book, deps.reads);
  if (st.kind === "not-deployed") return `The DatasetRegistry record waits: ${st.reason}. Core keeps the publication, and the record can be proposed once this network has DatasetRegistry.`;
  if (st.kind === "unknown") return `Could not read whether this network has DatasetRegistry (${st.reason}), so no registry record was proposed.`;
  const publisher = deps.publisher().trim();
  if (!/^0x[0-9a-fA-F]{40}$/.test(publisher)) return "This device's wallet address has not been read yet, so no registry record was proposed.";
  try {
    const id = datasetIdFor(publisher, name);
    const prev = await registryPrev(st.address, id, deps.reads);
    const calldata = publishCalldata(id, p, prev);
    await deps.propose({ address: st.address, calldata, valueWei: "0", label: PUBLISH_SIGNATURE });
    const note = prev.toLowerCase() !== p.registryPrev.toLowerCase() ? ` The registry's latest version differs from this device's record, so the call uses the registry's (${prev}).` : "";
    return `The DatasetRegistry record of version ${p.version} is waiting in the wallet review; the member signs it there. Nothing is recorded on chain until they approve.${note}`;
  } catch (e) {
    return "Core did not open the DatasetRegistry record: " + msg(e);
  }
}

export async function runPublishTool(args: Record<string, unknown>, ann: ToolAnnotation | null, deps: PublishToolDeps): Promise<PublishOutcome> {
  const id = typeof args.dataset === "string" ? args.dataset.trim() : "";
  const license = typeof args.license === "string" ? args.license : "";
  if (!DATASET_ID.test(id)) return { status: "ok", result: "fl_dataset_publish_propose needs the dataset id from fl_dataset_list. Nothing was proposed." };
  let ds;
  try {
    ds = (await deps.ds.list()).datasets.find((d) => d.id === id);
  } catch (e) {
    return { status: "ok", result: "couldn't read the datasets, so nothing was proposed: " + msg(e) };
  }
  if (!ds) return { status: "ok", result: `There is no dataset ${id} on this device. Nothing was proposed.` };
  if (!ds.chainOk) return { status: "ok", result: `Dataset ${id}'s version chain is broken (${ds.chainProblem ?? "unknown"}), so nothing was proposed.` };
  const v = latestVersion(ds);
  if (!v) return { status: "ok", result: `Dataset ${id} has no version to publish.` };
  // Already on IPFS: only the registry record can still be missing.
  let pubs: FlPublication[] = [];
  try {
    pubs = await deps.ds.published(id);
  } catch (e) {
    return { status: "ok", result: "couldn't read this dataset's publications, so nothing was proposed: " + msg(e) };
  }
  const done = pubs.find((p) => p.version === v.version);
  if (done) {
    return { status: "ok", result: `Version ${v.version} is already on IPFS at ${done.cid} (a CID cannot be recalled). ` + (await proposeRegistryRecord(ds.name, done, deps)) };
  }
  let check;
  try {
    check = await deps.ds.publishPreview(id, v.version, license, "public");
  } catch (e) {
    return { status: "ok", result: "Core could not check the publish, so nothing was proposed: " + msg(e) };
  }
  if (check.refusals.length) {
    return { status: "ok", result: `Core refuses publishing ${id} v${v.version}:\n${check.refusals.map((r) => "- " + r).join("\n")}\nNothing was proposed. Explain each reason to the member; you cannot override them.` };
  }
  const fields: Record<string, unknown> = {
    dataset: `${id} v${v.version}: ${check.rows} rows from ${check.sources} sources`,
    license: check.license,
    privacy: "Public: anyone with the CID can read every row",
    "PII scan": "passed on this device for exactly these rows",
    "cannot be undone": "a published CID cannot be recalled; deleting the dataset later does not remove it from IPFS",
    writes: `version ${v.version + 1} (the publish manifest), then adds it to IPFS on this node (${check.bundleBytes} bytes)`,
    "on chain": "nothing yet: a DatasetRegistry record, where this network has one, is a separate signature",
  };
  if (check.untrusted) fields.note = "some rows were written by other people (collected pages)";
  const spec: CerSpec = {
    origin: "chat agent",
    requester: "chat agent · dataset",
    title: "Publish a dataset to IPFS (cannot be undone)",
    chainless: true,
    rows: Object.entries(fields).map(([k, val]) => ({ k, v: String(val) })),
    cost: "none, no chain transaction",
    sponsor: "your explicit decision (HIC-1, never automatic)",
    sponsorColor: "var(--warn)",
  };
  const r = await deps.ask(spec, fieldsCard("fl_dataset_publish_propose", ann, fields, `publish dataset ${id} v${v.version} to IPFS`));
  if (r !== "approved") {
    return { status: r, result: r === "expired" ? "The approval expired because the run ended; nothing was published." : "The member declined publishing; nothing left this device." };
  }
  try {
    const out = await deps.ds.publish(id, v.version, check.license, "public", check.contentHash);
    const reg = await proposeRegistryRecord(out.name, out.publication, deps);
    return { status: "approved", result: `Published dataset ${id} version ${out.version.version} to IPFS on this node: CID ${out.publication.cid}, manifestHash ${out.publication.manifestHash}. A CID cannot be recalled. ${reg}` };
  } catch (e) {
    return { status: "error", result: "Core refused publishing: " + msg(e) };
  }
}
