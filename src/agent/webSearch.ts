// =====================================================================
// HUP-S5.1 / S5.2 / S5.3: the member's web opt-ins for Hermes, TypeScript side.
//
// Core (src-tauri/src/hermes_web.rs) stores the choices and turns them into the Hermes sidecar's
// environment when Hermes starts; the tools run in the sidecar (citrate-agent-runtime
// agent-search, agent-loop::decide). These are the wire types (camelCase, mirroring the Rust serde
// shapes) and the pure helpers the Settings card uses. Everything defaults to off / local.
// =====================================================================

export type ReaderChoice = "local" | "jina";

export interface HermesWebSettings {
  /** HUP-S5.1: Hermes's own headless browser (the `browser_*` tools). Off by default. */
  browserEnabled: boolean;
  searchEnabled: boolean;
  searxngPath: string | null;
  reader: ReaderChoice;
  jinaKeyFile: string | null;
  jevEnabled: boolean;
  jevKeyFile: string | null;
  jevOrigins: string[];
  jevNonWeb: boolean;
}

export interface HermesWebStatus {
  settings: HermesWebSettings;
  /** The managed Chromium's executable when the signed component is installed; null = not installed. */
  managedChromium: string | null;
  searxngFound: boolean;
  jinaKeyFileFound: boolean;
  jevKeyFileFound: boolean;
  notices: string[];
  appliesOnRestart: boolean;
  loadError?: string;
}

export const DEFAULT_WEB_SETTINGS: HermesWebSettings = {
  browserEnabled: false,
  searchEnabled: false,
  searxngPath: null,
  reader: "local",
  jinaKeyFile: null,
  jevEnabled: false,
  jevKeyFile: null,
  jevOrigins: [],
  jevNonWeb: false,
};

/** Mirrors `MAX_JEV_ORIGINS` in hermes_web.rs. */
export const MAX_JEV_ORIGINS = 32;

/** `https://host[:port]`, lowercased; `null` for anything else (paths, http, userinfo…). Mirrors
 *  `normalize_https_origin` in hermes_web.rs so the card can flag a bad line before saving. */
export function normalizeOrigin(raw: string): string | null {
  const s = raw.trim();
  if (s.slice(0, 8).toLowerCase() !== "https://") return null;
  let rest = s.slice(8);
  if (rest.endsWith("/")) rest = rest.slice(0, -1);
  if (!rest || /[/?#@\\\s]/.test(rest)) return null;
  return "https://" + rest.toLowerCase();
}

/** Parse the origins textarea (one per line or comma-separated), deduplicated, with bad lines. */
export function parseOrigins(text: string): { origins: string[]; bad: string[] } {
  const origins: string[] = [];
  const bad: string[] = [];
  for (const part of text.split(/[\n,]/)) {
    if (!part.trim()) continue;
    const n = normalizeOrigin(part);
    if (n === null) bad.push(part.trim());
    else if (!origins.includes(n)) origins.push(n);
  }
  return { origins, bad };
}

/** An absolute path (POSIX or Windows drive / UNC) or empty. Mirrors the Rust check closely enough
 *  to warn early; core is the authority. */
export function isAbsolutePath(p: string): boolean {
  const s = p.trim();
  return s.startsWith("/") || /^[A-Za-z]:[\\/]/.test(s) || s.startsWith("\\\\");
}

/** Blank strings become `null`; everything else is passed through trimmed. */
export function cleanSettings(s: HermesWebSettings): HermesWebSettings {
  const t = (v: string | null) => (v && v.trim() ? v.trim() : null);
  return { ...s, searxngPath: t(s.searxngPath), jinaKeyFile: t(s.jinaKeyFile), jevKeyFile: t(s.jevKeyFile) };
}

/** True when any choice sends data to a third party (the card shows a notice). */
export function sendsToThirdParty(s: HermesWebSettings): boolean {
  return (s.searchEnabled && s.reader === "jina") || s.jevEnabled;
}

/** Where the card talks to core. Injected so tests run without Tauri. */
export interface WebSettingsIo {
  mode: "tauri" | "sim";
  invoke: <T>(cmd: string, args?: Record<string, unknown>) => Promise<T>;
}

export async function loadWebSettings(io: WebSettingsIo): Promise<HermesWebStatus> {
  return io.invoke<HermesWebStatus>("hermes_web_settings_get");
}

export async function saveWebSettings(io: WebSettingsIo, settings: HermesWebSettings): Promise<HermesWebStatus> {
  return io.invoke<HermesWebStatus>("hermes_web_settings_set", { settings: cleanSettings(settings) });
}
