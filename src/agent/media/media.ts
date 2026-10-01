// =====================================================================
// HUP-S10.1 — media generation (webview half).
//
// Rust (src-tauri/src/media.rs) decides which routes exist on this device (tier), what each
// costs as far as the app can know, and where a file may be saved (a live write folder grant
// only). This module is the typed seam to those commands plus small display helpers.
// The web preview has no generator and no folders, so it says so (Rule 1).
// =====================================================================

export type MediaKind = "image" | "video";

/** One way to make media (mirrors Rust `Route`). */
export interface MediaRoute {
  id: "local" | "remote" | string;
  kind: MediaKind;
  available: boolean;
  reason: string | null;
  destination: string;
  cost: string;
  model: string;
}

export interface MediaSettings {
  localUrl: string | null;
  localModel: string | null;
  remoteProvider: string | null;
  remoteModel: string | null;
}

/** A write folder grant generated files may go to (mirrors Rust `Target`). */
export interface MediaTarget {
  grantId: string;
  root: string;
}

export interface MediaOptions {
  tier: string | null;
  caps: { localImage: boolean; localVideo: boolean };
  settings: MediaSettings;
  image: MediaRoute[];
  video: MediaRoute[];
  sizes: string[];
  targets: MediaTarget[];
  targetsError: string | null;
}

/** A saved item (mirrors Rust `GalleryItem` + `present`). */
export interface GalleryItem {
  id: string;
  kind: MediaKind;
  path: string;
  grantId: string;
  prompt: string;
  route: string;
  destination: string;
  model: string;
  createdAt: number;
  bytes: number;
  mime: string;
  cost: string;
  usage: Record<string, unknown> | null;
  present?: boolean;
}

export interface MediaIo {
  mode: "tauri" | "sim";
  invoke<T>(cmd: string, args: Record<string, unknown>): Promise<T>;
}

export const MEDIA_DESKTOP_ONLY = "Media generation needs the desktop app. The web preview has no generator and no granted folders.";

/** The usage line from a provider's report, or null when it reported none. */
export function usageLine(usage: Record<string, unknown> | null | undefined): string | null {
  if (!usage) return null;
  const total = usage.total_tokens;
  if (typeof total === "number" && Number.isFinite(total)) return `Provider reported ${total.toLocaleString("en-US")} tokens used.`;
  const parts = Object.entries(usage)
    .filter(([, v]) => typeof v === "number")
    .map(([k, v]) => `${k.replace(/_/g, " ")} ${(v as number).toLocaleString("en-US")}`);
  return parts.length ? `Provider reported: ${parts.join(", ")}.` : null;
}

/** File name from a full path, for compact display. */
export function baseName(path: string): string {
  const i = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  return i >= 0 ? path.slice(i + 1) : path;
}

export function errorMessage(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

/** The production IO: the bridge's runtime mode and the timeout-wrapped invoke. */
export async function desktopMediaIo(): Promise<MediaIo> {
  const { BRIDGE_MODE } = await import("../../bridge/mode");
  return {
    mode: BRIDGE_MODE,
    invoke: async <T,>(cmd: string, args: Record<string, unknown>) => {
      const { invoke } = await import("../../bridge/tauri/invoke");
      return invoke<T>(cmd, args);
    },
  };
}
