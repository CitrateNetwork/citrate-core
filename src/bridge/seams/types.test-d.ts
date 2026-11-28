// L0-WP05 type-level checks. `npm run typecheck` compiles this file (tsconfig includes src and
// excludes only *.test.ts / *.test.tsx), so a check that no longer holds fails the typecheck job.
// Nothing here runs; every export is a type.
import type { BridgeContract, CxBridge, ModelDescriptor } from "../domains";
import type { FullBridge } from "../index";
import type { V60Bridge } from "../lanes";

type Assert<T extends true> = T;
type Extends<A, B> = [A] extends [B] ? true : false;
type IsNever<T> = [T] extends [never] ? true : false;

/** The split changes no contract: the bridge is still the legacy contract plus the CX domains. */
export type FullBridgeExtendsLegacyAndCx = Assert<Extends<FullBridge, BridgeContract & CxBridge>>;
/** ...and it carries every v060 lane namespace. */
export type FullBridgeExtendsV60 = Assert<Extends<FullBridge, V60Bridge>>;
/** A lane key never shadows a legacy or CX domain (a later spread would silently replace it). */
export type LaneKeysAreNew = Assert<IsNever<keyof V60Bridge & keyof (BridgeContract & CxBridge)>>;
/** The Gherkin: a surface's `import type { ModelDescriptor } from "../bridge/domains"` still resolves. */
export type ModelDescriptorFromTheBarrel = Assert<Extends<ModelDescriptor["source"], "hf" | "github" | "bundled">>;
