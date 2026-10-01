// =====================================================================
// citrate-core — Media pop-out host, the main-window side (HUP-S10.1)
//
// Answers the Media player pop-out: the gallery and the folders a copy may be saved to when it is
// ready (and again whenever the main window makes something new), one image at a time as a data
// URL when asked, and "save a copy" through the Rust command. Testable on its own: the commands
// come in through `invoke`.
// =====================================================================
import type { BridgeTransport } from "./bridge";
import { createMediaMainEnd, type MediaCard, type MediaMainEnd } from "./mediaBridge";
import { baseName, errorMessage, usageLine, type GalleryItem, type MediaOptions } from "../agent/media/media";

export interface MediaHostDeps {
  transport: BridgeTransport;
  invoke<T>(cmd: string, args: Record<string, unknown>): Promise<T>;
}

export interface MediaHost {
  /** Push the current gallery to the pop-out (after something new was made). */
  refresh(): Promise<void>;
  dispose(): void;
}

export function toCard(i: GalleryItem): MediaCard {
  return {
    id: i.id,
    kind: i.kind,
    name: baseName(i.path),
    prompt: i.prompt,
    destination: i.destination,
    cost: i.cost,
    usage: usageLine(i.usage),
    createdAt: i.createdAt,
    present: i.present !== false,
  };
}

export async function createMediaHost(deps: MediaHostDeps): Promise<MediaHost> {
  let popoutReady = false;
  let disposed = false;
  let end: MediaMainEnd | null = null;

  const publish = async () => {
    if (disposed || !popoutReady || !end) return;
    let note: string | null = null;
    let items: MediaCard[] = [];
    let targets: MediaOptions["targets"] = [];
    try {
      items = (await deps.invoke<GalleryItem[]>("media_gallery", {})).map(toCard);
    } catch (e) {
      note = "The gallery could not be read: " + errorMessage(e);
    }
    try {
      const o = await deps.invoke<MediaOptions>("media_options", {});
      targets = o.targets;
      if (o.targetsError) note = o.targetsError;
    } catch (e) {
      note = note ?? "The granted folders could not be read: " + errorMessage(e);
    }
    await end.send({ v: 1, type: "media.gallery", items, targets, note }).catch(() => undefined);
  };

  end = await createMediaMainEnd(deps.transport, (m) => {
    if (disposed || !end) return;
    const e = end;
    if (m.type === "media.ready") {
      popoutReady = true;
      void publish();
    } else if (m.type === "media.show") {
      void deps
        .invoke<string>("media_read", { id: m.id })
        .then((dataUrl) => e.send({ v: 1, type: "media.image", id: m.id, dataUrl }))
        .catch((err) => e.send({ v: 1, type: "media.result", ok: false, message: errorMessage(err) }))
        .catch(() => undefined);
    } else {
      void deps
        .invoke<string>("media_save_copy", { id: m.id, grantId: m.grantId })
        .then((path) => e.send({ v: 1, type: "media.result", ok: true, message: "Saved a copy to " + path }))
        .catch((err) => e.send({ v: 1, type: "media.result", ok: false, message: errorMessage(err) }))
        .catch(() => undefined);
    }
  });

  return {
    refresh: publish,
    dispose() {
      disposed = true;
      end?.close();
    },
  };
}

let hostPromise: Promise<MediaHost> | null = null;

/** Start the main-window media host once (desktop app only). */
export function startMediaHost(): Promise<MediaHost> | null {
  hostPromise ??= (async () => {
    const { mediaTauriTransport } = await import("./mediaBridge");
    const { invoke } = await import("../bridge/tauri/invoke");
    return createMediaHost({ transport: await mediaTauriTransport(), invoke: (c, a) => invoke(c, a) });
  })().catch((e) => {
    hostPromise = null; // a later open tries again
    throw e;
  });
  return hostPromise;
}
