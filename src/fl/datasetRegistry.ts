// =====================================================================
// citrate-core 0.5.4 (WP-H6): the DatasetRegistry record of a published dataset version.
//
// DatasetRegistry (citrate-chain contracts/src/data/DatasetRegistry.sol, planset 06 §5) is merged
// but NOT deployed on r1007. The shipped address book (src-tauri/addresses/40204.json) does not pin
// it, so this module reports "Not available yet on this network" from the book and never builds a
// transaction. When a book pins it and the chain has code there, the same path builds the exact
// `publish(datasetId, Version)` call for the member to sign in the wallet review (HIC-1; Hermes
// never signs and the call is never inside an envelope).
//
// Data sources (Rule 7): the address book for the pin; `eth_getCode` for the code; the registry's
// own `versionCount` and `latest` views for the `prev` the contract will require.
// =====================================================================
import { decodeFunctionResult, encodeFunctionData, keccak256, toBytes, type Hex } from "viem";
import { NOT_AVAILABLE, pinOf, type FlBook } from "./networkState";
import type { FlPublication } from "./datasetTypes";

export const DATASET_REGISTRY_ABI = [
  {
    type: "function",
    name: "publish",
    stateMutability: "nonpayable",
    inputs: [
      { name: "datasetId", type: "bytes32" },
      {
        name: "v",
        type: "tuple",
        components: [
          { name: "publisher", type: "address" },
          { name: "manifestHash", type: "bytes32" },
          { name: "prev", type: "bytes32" },
          { name: "groupHash", type: "bytes32" },
          { name: "cid", type: "string" },
          { name: "license", type: "string" },
          { name: "privacy", type: "uint8" },
          { name: "publishedAt", type: "uint64" },
          { name: "withdrawn", type: "bool" },
        ],
      },
    ],
    outputs: [{ name: "version", type: "uint32" }],
  },
  { type: "function", name: "versionCount", stateMutability: "view", inputs: [{ name: "datasetId", type: "bytes32" }], outputs: [{ type: "uint32" }] },
  {
    type: "function",
    name: "latest",
    stateMutability: "view",
    inputs: [{ name: "datasetId", type: "bytes32" }],
    outputs: [
      { name: "version", type: "uint32" },
      {
        name: "v",
        type: "tuple",
        components: [
          { name: "publisher", type: "address" },
          { name: "manifestHash", type: "bytes32" },
          { name: "prev", type: "bytes32" },
          { name: "groupHash", type: "bytes32" },
          { name: "cid", type: "string" },
          { name: "license", type: "string" },
          { name: "privacy", type: "uint8" },
          { name: "publishedAt", type: "uint64" },
          { name: "withdrawn", type: "bool" },
        ],
      },
    ],
  },
] as const;

/** The signature the wallet review is titled with (its selector must match the calldata). */
export const PUBLISH_SIGNATURE = "publish(bytes32,(address,bytes32,bytes32,bytes32,string,string,uint8,uint64,bool))";

/** DatasetRegistry.Privacy: Public = 0 (this build publishes Public only). */
export const PRIVACY_PUBLIC = 0;

const ZERO32 = ("0x" + "00".repeat(32)) as Hex;
const ADDRESS = /^0x[0-9a-fA-F]{40}$/;

/** `datasetIdFor(publisher, name)`: the publisher in the top 20 bytes, the low 96 bits of keccak256(name) below. */
export function datasetIdFor(publisher: string, name: string): Hex {
  if (!ADDRESS.test(publisher)) throw new Error("the publisher is not an address");
  const low = BigInt(keccak256(toBytes(name))) & ((1n << 96n) - 1n);
  const id = (BigInt(publisher) << 96n) | low;
  return ("0x" + id.toString(16).padStart(64, "0")) as Hex;
}

export type RegistryState =
  | { kind: "not-deployed"; reason: string }
  | { kind: "unknown"; reason: string }
  | { kind: "deployed"; address: string };

export interface RegistryReads {
  codeSize: (address: string) => Promise<number>;
  viewCall: (address: string, calldata: string) => Promise<string>;
}

/** Whether this network has a DatasetRegistry, from the book and the chain. */
export async function registryState(book: FlBook, reads: RegistryReads): Promise<RegistryState> {
  const pin = pinOf(book, "DatasetRegistry");
  if (!pin) return { kind: "not-deployed", reason: NOT_AVAILABLE };
  try {
    return (await reads.codeSize(pin)) > 0 ? { kind: "deployed", address: pin } : { kind: "not-deployed", reason: NOT_AVAILABLE };
  } catch (e) {
    return { kind: "unknown", reason: e instanceof Error ? e.message : String(e) };
  }
}

/** The manifestHash the registry holds as the latest version of `datasetId`, or zero when none. */
export async function registryPrev(address: string, datasetId: Hex, reads: RegistryReads): Promise<Hex> {
  const countData = encodeFunctionData({ abi: DATASET_REGISTRY_ABI, functionName: "versionCount", args: [datasetId] });
  const count = decodeFunctionResult({ abi: DATASET_REGISTRY_ABI, functionName: "versionCount", data: (await reads.viewCall(address, countData)) as Hex });
  if (Number(count) === 0) return ZERO32;
  const latestData = encodeFunctionData({ abi: DATASET_REGISTRY_ABI, functionName: "latest", args: [datasetId] });
  const [, v] = decodeFunctionResult({ abi: DATASET_REGISTRY_ABI, functionName: "latest", data: (await reads.viewCall(address, latestData)) as Hex });
  return v.manifestHash;
}

/** The exact `publish` call for one publication. `prev` is what the registry holds now. */
export function publishCalldata(datasetId: Hex, p: FlPublication, prev: Hex): Hex {
  if (p.privacy !== "public") throw new Error("this build records Public datasets only");
  return encodeFunctionData({
    abi: DATASET_REGISTRY_ABI,
    functionName: "publish",
    args: [
      datasetId,
      {
        publisher: "0x0000000000000000000000000000000000000000",
        manifestHash: p.manifestHash as Hex,
        prev,
        groupHash: ZERO32,
        cid: p.cid,
        license: p.license,
        privacy: PRIVACY_PUBLIC,
        publishedAt: 0n,
        withdrawn: false,
      },
    ],
  });
}
