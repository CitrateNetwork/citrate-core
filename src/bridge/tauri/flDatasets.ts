// Bridge impl: the local FL dataset store (0.5.4 WP-H6), TAURI. Every call goes to core
// (src-tauri fl_datasets.rs); nothing is computed here.
import { invoke } from "./invoke";
import type {
  FlCleanOp,
  FlCleanPreview,
  FlConsentWithdrawn,
  FlDatasetConsent,
  FlDatasetCreateOutcome,
  FlDatasetCreateRequest,
  FlDatasetList,
  FlDatasetSample,
  FlDatasetsDomain,
  FlDatasetVersion,
  FlManifestView,
  FlPrivacyLabel,
  FlPublication,
  FlPublishPreview,
  FlPublished,
} from "../../fl/datasetTypes";

export const tauriFlDatasets: FlDatasetsDomain = {
  list() {
    return invoke<FlDatasetList>("fl_dataset_list");
  },
  manifest(dataset: string, version: number) {
    return invoke<FlManifestView>("fl_dataset_manifest", { dataset, version });
  },
  create(request: FlDatasetCreateRequest) {
    return invoke<FlDatasetCreateOutcome>("fl_dataset_create", { request });
  },
  sample(dataset: string, version: number, n?: number, offset?: number) {
    return invoke<FlDatasetSample>("fl_dataset_sample", { dataset, version, n: n ?? null, offset: offset ?? null });
  },
  cleanPreview(dataset: string, version: number, ops: FlCleanOp[]) {
    return invoke<FlCleanPreview>("fl_dataset_clean_preview", { dataset, version, ops });
  },
  cleanApply(dataset: string, version: number, ops: FlCleanOp[], resultContentHash: string) {
    return invoke<FlDatasetVersion>("fl_dataset_clean_apply", { dataset, version, ops, resultContentHash });
  },
  consentList(dataset: string) {
    return invoke<FlDatasetConsent[]>("fl_dataset_consent_list", { dataset });
  },
  consentWithdraw(dataset: string, consent: string) {
    return invoke<FlConsentWithdrawn>("fl_dataset_consent_withdraw", { dataset, consent });
  },
  async delete(dataset: string) {
    await invoke("fl_dataset_delete", { dataset });
  },
  publishPreview(dataset: string, version: number, license: string, privacy: FlPrivacyLabel) {
    return invoke<FlPublishPreview>("fl_dataset_publish_preview", { dataset, version, license, privacy });
  },
  publish(dataset: string, version: number, license: string, privacy: FlPrivacyLabel, contentHash: string) {
    return invoke<FlPublished>("fl_dataset_publish", { dataset, version, license, privacy, contentHash });
  },
  published(dataset: string) {
    return invoke<FlPublication[]>("fl_dataset_published", { dataset });
  },
};
