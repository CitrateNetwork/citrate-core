// D-11 (RC52) red stub: Settings › Models, not written yet (the next commit fills it in).
import type { ProviderStatus, ProviderUsageSummary } from "../../bridge/domains";
import type { CapsForm, CustomForm } from "./providerModels";

export type BusyOp = "connect" | "test" | "remove" | "revoke" | "halt" | "resume" | "caps" | "price" | "confirm" | "customRemove" | "custom" | "reset";

export interface PriceForm {
  model: string;
  inUsd: string;
  outUsd: string;
}

export interface ModelsActions {
  onKeyDraft(id: string, v: string): void;
  onConnect(id: string): void;
  onTest(id: string): void;
  onRemove(id: string): void;
  onRemoveAndRevoke(id: string): void;
  onOpen(url: string): void;
  onHalt(id: string): void;
  onResume(id: string): void;
  onEditCaps(id: string | null): void;
  onCapsForm(f: CapsForm): void;
  onSaveCaps(): void;
  onEditPrice(id: string | null): void;
  onPriceForm(f: PriceForm): void;
  onSavePrice(): void;
  onConfirmDraft(id: string, v: string): void;
  onConfirm(id: string): void;
  onRemoveCustom(id: string): void;
  onAdvanced(on: boolean): void;
  onCustomForm(f: CustomForm): void;
  onAddCustom(): void;
  onResetLog(): void;
}

export interface ModelsViewProps {
  desktop: boolean;
  statuses: ProviderStatus[] | null;
  usage: ProviderUsageSummary | null;
  /** Why the status or usage could not be read (shown as is). */
  loadError: string | null;
  nowMs: number;
  busy: { id: string; op: BusyOp } | null;
  /** Per provider id; the key "" is for page-level errors. */
  errors: Record<string, string>;
  /** Per provider id: the result of the last test or connect. */
  notices: Record<string, string>;
  keyDrafts: Record<string, string>;
  confirmDrafts: Record<string, string>;
  capsEdit: { id: string; form: CapsForm } | null;
  priceEdit: { id: string; form: PriceForm } | null;
  advanced: boolean;
  customForm: CustomForm;
  on: ModelsActions;
}

export function ModelsSettingsView(_v: ModelsViewProps) {
  return <div />;
}

export function ModelsSettings(_p: { onOpen(url: string): void }) {
  return <div />;
}
