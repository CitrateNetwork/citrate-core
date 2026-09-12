// CX bridge impl — social identity (Connections · social discovery), TAURI.
//
// The LINK flow is wired to the real Rust social_* commands (public-client loopback-PKCE, per
// ADR-2026-08-30): status / start (OAuth ownership proof → keyring-sealed token → device-local
// record) / setVisibility / disconnect. The token NEVER crosses this boundary. `verified` (the
// wallet-signed IdentityBinding through the ceremony, D3) is the follow-up — bindingChallenge/verify
// stay honest Unavailable until then. Rule 3 holds: when wired, the WALLET signs; no sidecar signs.
import { invoke } from "./invoke";
import type { DirectoryHit, DirectorySearchHit, ExportedBinding, LinkedIdentity, ResolvedIdentity, SocialDomain, SocialNetwork, SocialVisibility } from "../domains";
import type { CeremonyView } from "../types";

export const tauriSocial: SocialDomain = {
  status(): Promise<LinkedIdentity[]> {
    return invoke<LinkedIdentity[]>("social_status");
  },
  start(network: SocialNetwork): Promise<LinkedIdentity> {
    return invoke<LinkedIdentity>("social_start", { network });
  },
  verifyRequest(network: SocialNetwork): Promise<CeremonyView> {
    return invoke<CeremonyView>("social_verify_request", { network });
  },
  verifyApprove(id: string, rawAck: boolean): Promise<LinkedIdentity> {
    return invoke<LinkedIdentity>("social_verify_approve", { id, rawAck });
  },
  async verifyForget(id: string): Promise<void> {
    await invoke("social_verify_forget", { id });
  },
  setVisibility(network: SocialNetwork, visibility: SocialVisibility): Promise<LinkedIdentity> {
    return invoke<LinkedIdentity>("social_set_visibility", { network, visibility });
  },
  async disconnect(network: SocialNetwork): Promise<void> {
    await invoke("social_disconnect", { network });
  },
  resolve(addresses: string[]): Promise<ResolvedIdentity[]> {
    return invoke<ResolvedIdentity[]>("social_resolve", { addresses });
  },
  exportBinding(network: SocialNetwork): Promise<ExportedBinding | null> {
    return invoke<ExportedBinding | null>("social_export_binding", { network });
  },
  ingestBinding(sender: string, binding: ExportedBinding): Promise<boolean> {
    return invoke<boolean>("social_ingest_binding", { sender, binding });
  },
  directoryPublishRequest(network: SocialNetwork): Promise<CeremonyView> {
    return invoke<CeremonyView>("directory_publish_request", { network });
  },
  directoryPublishApprove(id: string, rawAck: boolean): Promise<LinkedIdentity> {
    return invoke<LinkedIdentity>("directory_publish_approve", { id, rawAck });
  },
  directoryUnpublishRequest(network: SocialNetwork): Promise<CeremonyView> {
    return invoke<CeremonyView>("directory_unpublish_request", { network });
  },
  directoryUnpublishApprove(id: string, rawAck: boolean): Promise<LinkedIdentity> {
    return invoke<LinkedIdentity>("directory_unpublish_approve", { id, rawAck });
  },
  async directoryForget(id: string): Promise<void> {
    await invoke("directory_forget", { id });
  },
  directoryLookup(platform: SocialNetwork, handle: string): Promise<DirectoryHit | null> {
    return invoke<DirectoryHit | null>("directory_lookup", { platform, handle });
  },
  directorySearch(platform: SocialNetwork, query: string): Promise<DirectorySearchHit[]> {
    return invoke<DirectorySearchHit[]>("directory_search", { platform, query });
  },
};
