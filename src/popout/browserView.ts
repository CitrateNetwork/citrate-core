// =====================================================================
// citrate-core — the Browser view's data (HUP-S5.1 + S5.6)
//
// What the Browser pop-out and the main window's browser controls read: the Hermes sidecar's
// browser status and its latest screencast frame. Both come through Rust commands and are checked
// here before they are shown or sent over the pop-out bridge: a frame's pixels must be plain
// base64 JPEG (they become a data: URL), malformed parts are dropped, and a status that cannot be
// read is shown as "off", never as a working browser (Rule 1).
// =====================================================================

export type ChromiumState = { state: "managed" | "system" | "not_installed"; path: string | null; searched: number | null };
export type BrowserMode = "off" | "managed" | "attached";
export type PendingBrowserAction = { id: string; tool: string; summary: string; reason: string };
export type ConsentNeeded = { origin: string; category: string | null };
export type ExcludedCategory = { id: string; label: string };
/** HUP-S2.3: a page in the managed browser asking for the member's address or a sign-in. */
export type SignInRequestView = { id: string; kind: "accounts" | "personal_sign"; raiseOrigin: string; topFrame: boolean };

export type BrowserState = {
  enabled: boolean;
  /** False when Hermes itself is not running; null when unknown. */
  running: boolean | null;
  chromium: ChromiumState | null;
  mode: BrowserMode;
  attachPort: number | null;
  stopped: boolean;
  url: string;
  consentedOrigins: string[];
  excludedCategories: ExcludedCategory[];
  consentNeeded: ConsentNeeded | null;
  pendingAction: PendingBrowserAction | null;
  /** HUP-S2.3: sign-in requests waiting for core (managed browser only). */
  signInRequests: SignInRequestView[];
};

export type Highlight = { ref: string; label: string; x: number; y: number; width: number; height: number; state: "pending" | "acted" };

export type BrowserFrame = {
  version: number;
  mime: "image/jpeg";
  data: string;
  viewportWidth: number;
  viewportHeight: number;
  url: string;
  withheld: boolean;
  highlight: Highlight | null;
};

export type BrowserView = { state: BrowserState; frame: BrowserFrame | null };

export const BROWSER_OFF: BrowserState = Object.freeze({
  enabled: false,
  running: null,
  chromium: null,
  mode: "off",
  attachPort: null,
  stopped: false,
  url: "",
  consentedOrigins: [],
  excludedCategories: [],
  consentNeeded: null,
  pendingAction: null,
  signInRequests: [],
}) as BrowserState;

/** The largest frame accepted (base64 characters). */
export const MAX_FRAME_CHARS = 16 * 1024 * 1024;
const MAX_TEXT = 4096;
const BASE64 = /^[A-Za-z0-9+/]*={0,2}$/;

const isObj = (v: unknown): v is Record<string, unknown> => typeof v === "object" && v !== null && !Array.isArray(v);
const str = (v: unknown, max = MAX_TEXT): string | null => (typeof v === "string" && v.length <= max ? v : null);
const num = (v: unknown): number | null => (typeof v === "number" && Number.isFinite(v) ? v : null);

function parseChromium(raw: unknown): ChromiumState | null {
  if (!isObj(raw)) return null;
  const state = raw.state;
  if (state !== "managed" && state !== "system" && state !== "not_installed") return null;
  return {
    state,
    path: str(raw.path),
    searched: Array.isArray(raw.searched) ? raw.searched.length : null,
  };
}

function parsePending(raw: unknown): PendingBrowserAction | null {
  if (!isObj(raw)) return null;
  const id = str(raw.id, 32);
  const tool = str(raw.tool, 64);
  const summary = str(raw.summary);
  const reason = str(raw.reason) ?? "";
  if (!id || !tool || summary === null) return null;
  return { id, tool, summary, reason };
}

const SIGN_IN_ID = /^signin-[0-9-]{1,56}$/;

function parseSignInRequests(raw: unknown): SignInRequestView[] {
  if (!Array.isArray(raw)) return [];
  return raw.slice(0, 8).flatMap((r) => {
    if (!isObj(r)) return [];
    const id = str(r.id, 64);
    const origin = str(r.raiseOrigin, 300) ?? "";
    if (!id || !SIGN_IN_ID.test(id)) return [];
    if (r.kind !== "accounts" && r.kind !== "personal_sign") return [];
    return [{ id, kind: r.kind, raiseOrigin: origin, topFrame: r.topFrame === true }];
  });
}

function parseConsentNeeded(raw: unknown): ConsentNeeded | null {
  if (!isObj(raw)) return null;
  const origin = str(raw.origin);
  if (!origin) return null;
  return { origin, category: str(raw.category, 64) };
}

/** The sidecar's browser status (via `hermes_browser_status`). Unknown → off. */
export function parseBrowserStatus(raw: unknown): BrowserState {
  if (!isObj(raw) || typeof raw.enabled !== "boolean") return BROWSER_OFF;
  const running = typeof raw.running === "boolean" ? raw.running : null;
  if (!raw.enabled) return { ...BROWSER_OFF, running };
  const mode: BrowserMode = raw.mode === "managed" || raw.mode === "attached" ? raw.mode : "off";
  const origins = Array.isArray(raw.consentedOrigins) ? raw.consentedOrigins.filter((o): o is string => typeof o === "string" && o.length <= MAX_TEXT) : [];
  const cats = Array.isArray(raw.excludedCategories)
    ? raw.excludedCategories.flatMap((c) => {
        if (!isObj(c)) return [];
        const id = str(c.id, 64);
        const label = str(c.label, 200);
        return id && label ? [{ id, label }] : [];
      })
    : [];
  const port = num(raw.attachPort);
  return {
    enabled: true,
    running: true,
    chromium: parseChromium(raw.chromium),
    mode,
    attachPort: port !== null && Number.isInteger(port) && port > 0 && port < 65536 ? port : null,
    stopped: raw.stopped === true,
    url: str(raw.url) ?? "",
    consentedOrigins: origins,
    excludedCategories: cats,
    consentNeeded: parseConsentNeeded(raw.consentNeeded),
    pendingAction: parsePending(raw.pendingAction),
    signInRequests: parseSignInRequests(raw.signInRequests),
  };
}

function parseHighlight(raw: unknown): Highlight | null {
  if (!isObj(raw)) return null;
  const ref = str(raw.ref, 16);
  const label = str(raw.label, 400) ?? "";
  const [x, y, width, height] = [num(raw.x), num(raw.y), num(raw.width), num(raw.height)];
  if (!ref || x === null || y === null || width === null || height === null) return null;
  const state = raw.state === "pending" ? "pending" : "acted";
  return { ref, label, x, y, width, height, state };
}

/** One screencast view (via `hermes_browser_frame`), or null when it is malformed. */
export function parseBrowserFrame(raw: unknown): BrowserFrame | null {
  if (!isObj(raw)) return null;
  const version = num(raw.version);
  if (version === null || version < 0 || !Number.isInteger(version)) return null;
  if (raw.mime !== "image/jpeg") return null;
  const data = typeof raw.data === "string" ? raw.data : null;
  if (data === null || data.length > MAX_FRAME_CHARS || !BASE64.test(data)) return null;
  const vw = num(raw.viewportWidth);
  const vh = num(raw.viewportHeight);
  if (vw === null || vh === null) return null;
  return {
    version,
    mime: "image/jpeg",
    data,
    viewportWidth: vw,
    viewportHeight: vh,
    url: str(raw.url) ?? "",
    withheld: raw.withheld === true,
    highlight: parseHighlight(raw.highlight),
  };
}

/** A view received over the pop-out bridge, re-checked whole. */
export function parseBrowserView(raw: unknown): BrowserView | null {
  if (!isObj(raw) || !isObj(raw.state)) return null;
  const state = parseBrowserStatus(raw.state);
  if (raw.frame === null || raw.frame === undefined) return { state, frame: null };
  const frame = parseBrowserFrame(raw.frame);
  return frame ? { state, frame } : null;
}

/** The frame as an image URL, or null when there is nothing to show. */
export function frameSrc(f: BrowserFrame | null): string | null {
  if (!f || f.withheld || f.data.length === 0) return null;
  return `data:${f.mime};base64,${f.data}`;
}

const pct = (n: number) => `${Math.round(n * 10000) / 100}%`;

/** Where to draw the outline of the element Hermes is acting on, as shares of the frame. */
export function highlightBox(f: BrowserFrame | null): { left: string; top: string; width: string; height: string; label: string; pending: boolean } | null {
  if (!f || !f.highlight || f.viewportWidth <= 0 || f.viewportHeight <= 0) return null;
  const h = f.highlight;
  return {
    left: pct(h.x / f.viewportWidth),
    top: pct(h.y / f.viewportHeight),
    width: pct(h.width / f.viewportWidth),
    height: pct(h.height / f.viewportHeight),
    label: `[${h.ref}] ${h.label}`,
    pending: h.state === "pending",
  };
}
