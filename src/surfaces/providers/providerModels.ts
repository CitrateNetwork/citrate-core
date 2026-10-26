// D-11 (RC52) red stub: the Settings › Models rules, not written yet (the next commit fills them in).
import type { ProviderCaps, ProviderPeriodUsage, ProviderStatus, ProviderUsage } from "../../bridge/domains";

export const ROTATE_AFTER_DAYS = 0;
export type CardState = "unavailable" | "unconfirmed" | "halted" | "capped" | "connected" | "disconnected";
export const capReached = (_u: ProviderUsage | undefined, _c: ProviderCaps): string | null => null;
export const cardState = (_p: ProviderStatus, _u: ProviderUsage | undefined): CardState => "disconnected";
export const keyAgeDays = (_k: number | null, _n: number): number | null => null;
export const rotationDue = (_p: ProviderStatus, _n: number): number | null => null;
export const resumeEffect = (_k: string) => ({ label: "", note: "" });
export const fmtTokens = (n: number) => String(n);
export const fmtWhen = (_ms: number) => "";
export const fmtDay = (_ms: number) => "";
export const tokenLine = (_u: number, _c: number | null) => "";
export const spendLine = (_p: ProviderPeriodUsage, _c: number | null) => "";
export interface CapsForm {
  dailyTokens: string;
  monthlyTokens: string;
  dailyUsd: string;
  monthlyUsd: string;
}
export const capsToForm = (_c: ProviderCaps): CapsForm => ({ dailyTokens: "", monthlyTokens: "", dailyUsd: "", monthlyUsd: "" });
export const formToCaps = (_f: CapsForm): { caps: ProviderCaps } | { error: string } => ({ error: "" });
export interface CustomForm {
  label: string;
  baseUrl: string;
  models: string;
}
export const EMPTY_CUSTOM: CustomForm = { label: "", baseUrl: "", models: "" };
export const customToInput = (_f: CustomForm): { label: string; baseUrl: string; models: string[] } | { error: string } => ({ error: "" });
export const confirmMatches = (_t: string, _b: string) => false;
export const hygieneSteps = (_n: string, _h: string): string[] => [];
export const VENDOR_HINT: Record<string, string> = {};
export const VENDOR_SUBTITLE: Record<string, string> = {};
