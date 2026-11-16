// Bridge impl: the local FL dataset store (0.5.4 WP-H6), SIM (web/dev). A browser preview has no
// sealed store and no member folders, so every call says so instead of inventing a dataset (Rule 1).
import type { FlDatasetsDomain } from "../../fl/datasetTypes";
import type { SimHost } from "./index";

export const DATASETS_NEED_DESKTOP = "datasets need the desktop app; nothing was read or written";

export function simFlDatasets(_host: SimHost): FlDatasetsDomain {
  const refuse = async (): Promise<never> => {
    throw new Error(DATASETS_NEED_DESKTOP);
  };
  return {
    list: refuse,
    manifest: refuse,
    create: refuse,
    sample: refuse,
    cleanPreview: refuse,
    cleanApply: refuse,
    consentList: refuse,
    consentWithdraw: refuse,
    delete: refuse,
    publishPreview: refuse,
    publish: refuse,
    published: refuse,
  };
}
