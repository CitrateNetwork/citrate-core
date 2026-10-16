// =====================================================================
// citrate-core — Media pop-out message bridge (HUP-S10.1)
//
// The Media player pop-out holds no app commands (capabilities/popout.json grants only bridge
// events), so everything it shows comes from the main window over this typed channel, on its own
// event name so the Activity monitor's channel is untouched. Every message carries a version and
// is validated on receipt; anything malformed is dropped whole. An image arrives only as a
// `data:` URL of PNG, JPEG or WebP (the main window re-checks the file before sending it).
// =====================================================================
import type { BridgeTransport } from "./bridge";
import { MAIN_LABEL } from "./bridge";
import { popoutLabel } from "./kinds";

export const MEDIA_EVENT = "citrate-media";

/** What the pop-out lists (a subset of the gallery item: no prompt-independent internals). */
export interface MediaCard {
  id: string;
  kind: "image" | "video";
  name: string;
  prompt: string;
  destination: string;
  cost: string;
  usage: string | null;
  createdAt: number;
  present: boolean;
}

export interface MediaSaveTarget {
  grantId: string;
  root: string;
}

export type ToMainMedia =
  | { v: 1; type: "media.ready" }
  | { v: 1; type: "media.show"; id: string }
  | { v: 1; type: "media.save"; id: string; grantId: string };

export type ToMediaPopout =
  | { v: 1; type: "media.gallery"; items: MediaCard[]; targets: MediaSaveTarget[]; note: string | null }
  | { v: 1; type: "media.image"; id: string; dataUrl: string }
  | { v: 1; type: "media.result"; ok: boolean; message: string };

const isObj = (v: unknown): v is Record<string, unknown> => typeof v === "object" && v !== null && !Array.isArray(v);
const isStr = (v: unknown, max = 4096): v is string => typeof v === "string" && v.length <= max;
const ID = /^[A-Za-z0-9_-]{1,64}$/;
const DATA_URL = /^data:image\/(png|jpeg|webp);base64,[A-Za-z0-9+/]+=*$/;
/** 25 MiB of image as base64, plus the prefix. */
const MAX_DATA_URL = Math.ceil((25 * 1024 * 1024) / 3) * 4 + 64;

export function parseToMainMedia(raw: unknown): ToMainMedia | null {
  if (!isObj(raw) || raw.v !== 1) return null;
  if (raw.type === "media.ready") return { v: 1, type: "media.ready" };
  if (raw.type === "media.show" && isStr(raw.id) && ID.test(raw.id)) return { v: 1, type: "media.show", id: raw.id };
  if (raw.type === "media.save" && isStr(raw.id) && ID.test(raw.id) && isStr(raw.grantId) && ID.test(raw.grantId)) {
    return { v: 1, type: "media.save", id: raw.id, grantId: raw.grantId };
  }
  return null;
}

function isCard(c: unknown): c is MediaCard {
  return (
    isObj(c) &&
    isStr(c.id) &&
    ID.test(c.id) &&
    (c.kind === "image" || c.kind === "video") &&
    isStr(c.name, 512) &&
    isStr(c.prompt, 8192) &&
    isStr(c.destination, 512) &&
    isStr(c.cost, 512) &&
    (c.usage === null || isStr(c.usage, 512)) &&
    typeof c.createdAt === "number" &&
    typeof c.present === "boolean"
  );
}

function isTarget(t: unknown): t is MediaSaveTarget {
  return isObj(t) && isStr(t.grantId) && ID.test(t.grantId) && isStr(t.root, 4096);
}

export function parseToMediaPopout(raw: unknown): ToMediaPopout | null {
  if (!isObj(raw) || raw.v !== 1) return null;
  if (raw.type === "media.gallery") {
    if (!Array.isArray(raw.items) || raw.items.length > 1000 || !raw.items.every(isCard)) return null;
    if (!Array.isArray(raw.targets) || raw.targets.length > 200 || !raw.targets.every(isTarget)) return null;
    if (!(raw.note === null || isStr(raw.note, 1024))) return null;
    return { v: 1, type: "media.gallery", items: raw.items, targets: raw.targets, note: raw.note };
  }
  if (raw.type === "media.image") {
    if (!isStr(raw.id) || !ID.test(raw.id)) return null;
    if (typeof raw.dataUrl !== "string" || raw.dataUrl.length > MAX_DATA_URL || !DATA_URL.test(raw.dataUrl)) return null;
    return { v: 1, type: "media.image", id: raw.id, dataUrl: raw.dataUrl };
  }
  if (raw.type === "media.result" && typeof raw.ok === "boolean" && isStr(raw.message, 1024)) {
    return { v: 1, type: "media.result", ok: raw.ok, message: raw.message };
  }
  return null;
}

/** The Tauri transport on the media event (desktop app only). */
export async function mediaTauriTransport(): Promise<BridgeTransport> {
  const { emitTo } = await import("@tauri-apps/api/event");
  const { getCurrentWebviewWindow } = await import("@tauri-apps/api/webviewWindow");
  const self = getCurrentWebviewWindow();
  return {
    send: (target, payload) => emitTo(target, MEDIA_EVENT, payload),
    listen: (handler) => self.listen<unknown>(MEDIA_EVENT, (e) => handler(e.payload)),
  };
}

export interface MediaPopoutEnd {
  ready(): Promise<void>;
  show(id: string): Promise<void>;
  save(id: string, grantId: string): Promise<void>;
  close(): void;
}

export async function createMediaPopoutEnd(t: BridgeTransport, onMsg: (m: ToMediaPopout) => void): Promise<MediaPopoutEnd> {
  let open = true;
  const unlisten = await t.listen((raw) => {
    if (!open) return;
    const m = parseToMediaPopout(raw);
    if (m) onMsg(m);
  });
  const send = (m: ToMainMedia) => (open ? t.send(MAIN_LABEL, m) : Promise.resolve());
  return {
    ready: () => send({ v: 1, type: "media.ready" }),
    show: (id) => send({ v: 1, type: "media.show", id }),
    save: (id, grantId) => send({ v: 1, type: "media.save", id, grantId }),
    close() {
      open = false;
      unlisten();
    },
  };
}

export interface MediaMainEnd {
  send(m: ToMediaPopout): Promise<void>;
  close(): void;
}

export async function createMediaMainEnd(t: BridgeTransport, onMsg: (m: ToMainMedia) => void): Promise<MediaMainEnd> {
  let open = true;
  const unlisten = await t.listen((raw) => {
    if (!open) return;
    const m = parseToMainMedia(raw);
    if (m) onMsg(m);
  });
  return {
    send: (m) => (open ? t.send(popoutLabel("media"), m) : Promise.resolve()),
    close() {
      open = false;
      unlisten();
    },
  };
}
