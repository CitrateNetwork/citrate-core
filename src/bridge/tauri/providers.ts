// Bridge impl — cloud model providers (D-11), TAURI. Every command runs in Rust `providers_ipc.rs`
// on the core model proxy: keys go in once (tested live, then sealed in the OS keychain) and never
// come back; caps, halts and custom endpoints live in core's sealed guard file; usage comes from
// the signed usage log.
import { invoke } from "./invoke";
import type {
  ProviderCaps,
  ProviderCustomEndpoint,
  ProviderKeyTest,
  ProviderPrice,
  ProvidersDomain,
  ProviderStatus,
  ProviderUsageSummary,
} from "../domains";

export const tauriProviders: ProvidersDomain = {
  status() {
    return invoke<ProviderStatus[]>("providers_status");
  },
  usageSummary() {
    return invoke<ProviderUsageSummary>("providers_usage_summary");
  },
  setCaps(providerId: string, caps: ProviderCaps) {
    return invoke<ProviderStatus>("providers_caps_set", { providerId, caps });
  },
  setPrice(providerId: string, model: string, price: ProviderPrice | null) {
    return invoke<ProviderStatus>("providers_price_set", { providerId, model, price });
  },
  halt(providerId: string) {
    return invoke<ProviderStatus>("providers_halt", { providerId });
  },
  resume(providerId: string) {
    return invoke<ProviderStatus>("providers_resume", { providerId });
  },
  setKey(providerId: string, apiKey: string) {
    return invoke<ProviderKeyTest>("providers_key_set", { providerId, apiKey });
  },
  testKey(providerId: string) {
    return invoke<ProviderKeyTest>("providers_key_test", { providerId });
  },
  removeKey(providerId: string) {
    return invoke<void>("providers_key_remove", { providerId });
  },
  addCustom(label: string, baseUrl: string, models: string[]) {
    return invoke<ProviderCustomEndpoint>("providers_custom_add", { label, baseUrl, models });
  },
  confirmCustom(id: string, baseUrl: string) {
    return invoke<ProviderCustomEndpoint>("providers_custom_confirm", { id, baseUrl });
  },
  removeCustom(id: string) {
    return invoke<void>("providers_custom_remove", { id });
  },
  resetUsageLog() {
    return invoke<ProviderUsageSummary>("providers_usage_log_reset");
  },
};
