// HUP-S10.1 (US-10.1) — the Media panel, the Media pop-out bridge, its main-window host, and the
// Media player view.
//
// BDD:
//   AC1 tiered local vs registry/endpoint: "lists every route with its availability, reason and cost".
//   AC2 outputs open in the Media pop-out: "the host answers ready with the gallery",
//       "the player shows the newest image and saves a copy to a granted folder".
//   AC3 cost is shown: "shows the cost of the chosen route before generating" and the player's cost line.
//   Granted folders only: "without a write grant there is nowhere to save, and Generate stays off".
import { describe, it, expect, vi } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { MediaPanel } from "./MediaPanel";
import { baseName, usageLine, type GalleryItem, type MediaIo, type MediaOptions } from "./media";
import { parseToMainMedia, parseToMediaPopout, createMediaPopoutEnd, type ToMediaPopout } from "../../popout/mediaBridge";
import { createMediaHost, toCard } from "../../popout/mediaHost";
import { MediaPlayer } from "../../popout/MediaPlayer";
import { PopoutRoot } from "../../popout/PopoutRoot";
import type { BridgeTransport } from "../../popout/bridge";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const PNG = "data:image/png;base64,iVBORw0KGgo=";

function options(over: Partial<MediaOptions> = {}): MediaOptions {
  return {
    tier: "T1",
    caps: { localImage: true, localVideo: false },
    settings: { localUrl: "http://127.0.0.1:7860/v1", localModel: "sd-turbo", remoteProvider: null, remoteModel: null },
    image: [
      { id: "local", kind: "image", available: true, reason: null, destination: "this device (127.0.0.1:7860)", cost: "No charge: runs on this device", model: "sd-turbo" },
      { id: "remote", kind: "image", available: false, reason: "There is no image provider chosen.", destination: "a provider", cost: "Billed by the provider to your key", model: "" },
    ],
    video: [{ id: "local", kind: "video", available: false, reason: "Video generation is not available on this device, and there is no video backend in this build yet.", destination: "this device", cost: "No charge: would run on this device", model: "" }],
    sizes: ["512x512", "1024x1024"],
    targets: [{ grantId: "4", root: "/Users/m/Pictures" }],
    targetsError: null,
    ...over,
  };
}

function item(over: Partial<GalleryItem> = {}): GalleryItem {
  return {
    id: "m1790812800-ab12cd34",
    kind: "image",
    path: "/Users/m/Pictures/citrate-image-20261001-000000-ab12cd34.png",
    grantId: "4",
    prompt: "a lemon tree at dusk",
    route: "local",
    destination: "this device (127.0.0.1:7860)",
    model: "sd-turbo",
    createdAt: 1_790_812_800,
    bytes: 1234,
    mime: "image/png",
    cost: "No charge: runs on this device",
    usage: null,
    present: true,
    ...over,
  };
}

type Calls = Array<[string, Record<string, unknown>]>;
function makeIo(handler: (cmd: string, args: Record<string, unknown>) => unknown, mode: "tauri" | "sim" = "tauri") {
  const calls: Calls = [];
  const io: MediaIo = {
    mode,
    invoke: async <T,>(cmd: string, args: Record<string, unknown>) => {
      calls.push([cmd, args]);
      const r = handler(cmd, args);
      if (r instanceof Error) throw r;
      return r as T;
    },
  };
  return { io, calls };
}

async function mount(el: React.ReactElement): Promise<{ host: HTMLDivElement; root: Root }> {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(el);
  });
  await act(async () => {});
  await act(async () => {});
  return { host, root };
}
const q = <T extends Element = HTMLElement>(host: HTMLElement, id: string) => host.querySelector(`[data-testid="${id}"]`) as T | null;
async function click(el: Element | null) {
  expect(el).toBeTruthy();
  await act(async () => {
    (el as HTMLElement).click();
  });
  await act(async () => {});
  await act(async () => {});
}
async function type(el: HTMLTextAreaElement | HTMLInputElement | null, v: string) {
  expect(el).toBeTruthy();
  await act(async () => {
    const proto = el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value")!.set!.call(el, v);
    el!.dispatchEvent(new Event("input", { bubbles: true }));
  });
}

function bus() {
  const listeners = new Map<string, ((p: unknown) => void)[]>();
  const transport = (self: string): BridgeTransport => ({
    async send(to, payload) {
      for (const f of listeners.get(to) ?? []) f(JSON.parse(JSON.stringify(payload)));
    },
    async listen(handler) {
      listeners.set(self, [...(listeners.get(self) ?? []), handler]);
      return () => listeners.set(self, (listeners.get(self) ?? []).filter((f) => f !== handler));
    },
  });
  return { transport };
}

describe("media helpers", () => {
  it("formats provider usage honestly and file names compactly", () => {
    expect(usageLine(null)).toBeNull();
    expect(usageLine({ total_tokens: 4200 })).toBe("Provider reported 4,200 tokens used.");
    expect(usageLine({ input_tokens: 10, output_tokens: 5 })).toBe("Provider reported: input tokens 10, output tokens 5.");
    expect(usageLine({ note: "x" })).toBeNull();
    expect(baseName("/a/b/c.png")).toBe("c.png");
    expect(baseName("C:\\x\\y.png")).toBe("y.png");
  });
});

describe("HUP-S10.1 MediaPanel", () => {
  it("in the web preview says media needs the desktop app and calls nothing", async () => {
    const { io, calls } = makeIo(() => options(), "sim");
    const { host } = await mount(<MediaPanel io={async () => io} openPlayer={() => undefined} />);
    expect(q(host, "media-desktop-only")?.textContent).toContain("desktop app");
    expect(calls).toEqual([]);
  });

  it("lists every route with its availability, reason and cost", async () => {
    const { io } = makeIo(() => options());
    const { host } = await mount(<MediaPanel io={async () => io} openPlayer={() => undefined} />);
    expect(q(host, "media-tier")?.textContent).toContain("T1");
    const local = q(host, "media-route-image-local")!.textContent!;
    expect(local).toContain("available");
    expect(local).toContain("No charge");
    const remote = q(host, "media-route-image-remote")!.textContent!;
    expect(remote).toContain("not available");
    expect(remote).toContain("no image provider");
    expect(q(host, "media-route-video-local")!.textContent).toContain("no video backend");
  });

  it("shows the cost of the chosen route before generating, then saves into the granted folder", async () => {
    const onSaved = vi.fn();
    const { io, calls } = makeIo((cmd) => (cmd === "media_options" ? options() : cmd === "media_generate_image" ? item() : cmd === "media_read" ? PNG : null));
    const { host } = await mount(<MediaPanel io={async () => io} openPlayer={() => undefined} onSaved={onSaved} />);
    expect(q(host, "media-cost-now")?.textContent).toContain("No charge");
    expect(q<HTMLButtonElement>(host, "media-generate")!.disabled).toBe(true);
    await type(q<HTMLTextAreaElement>(host, "media-prompt"), "a lemon tree at dusk");
    expect(q<HTMLButtonElement>(host, "media-generate")!.disabled).toBe(false);
    await click(q(host, "media-generate"));
    const gen = calls.find(([c]) => c === "media_generate_image")!;
    expect(gen[1]).toEqual({ route: "local", prompt: "a lemon tree at dusk", size: "1024x1024", grantId: "4" });
    expect(q(host, "media-last")?.textContent).toContain("citrate-image-20261001-000000-ab12cd34.png");
    expect(q(host, "media-last")?.querySelector("img")?.getAttribute("src")).toBe(PNG);
    expect(onSaved).toHaveBeenCalledTimes(1);
  });

  it("without a write grant there is nowhere to save, and Generate stays off", async () => {
    const { io } = makeIo(() => options({ targets: [] }));
    const { host } = await mount(<MediaPanel io={async () => io} openPlayer={() => undefined} />);
    expect(q(host, "media-no-target")?.textContent).toContain("folder you granted");
    await type(q<HTMLTextAreaElement>(host, "media-prompt"), "anything");
    expect(q<HTMLButtonElement>(host, "media-generate")!.disabled).toBe(true);
  });

  it("with no available route, Generate stays off and a refusal from Rust is shown", async () => {
    const none = options();
    none.image = none.image.map((r) => ({ ...r, available: false, reason: "nope" }));
    const { io } = makeIo(() => none);
    const { host } = await mount(<MediaPanel io={async () => io} openPlayer={() => undefined} />);
    await type(q<HTMLTextAreaElement>(host, "media-prompt"), "anything");
    expect(q<HTMLButtonElement>(host, "media-generate")!.disabled).toBe(true);

    const fail = makeIo((cmd) => (cmd === "media_options" ? options() : new Error("the local image server answered HTTP 500")));
    const m = await mount(<MediaPanel io={async () => fail.io} openPlayer={() => undefined} />);
    await type(q<HTMLTextAreaElement>(m.host, "media-prompt"), "x");
    await click(q(m.host, "media-generate"));
    expect(q(m.host, "media-error")?.textContent).toContain("HTTP 500");
  });

  it("saves media settings through Rust and opens the player on request", async () => {
    const openPlayer = vi.fn();
    const { io, calls } = makeIo((cmd, a) => (cmd === "media_options" ? options() : cmd === "media_set_settings" ? a.settings : null));
    const { host } = await mount(<MediaPanel io={async () => io} openPlayer={openPlayer} />);
    await click(q(host, "media-settings-edit"));
    await click(q(host, "media-settings-save"));
    expect(calls.some(([c]) => c === "media_set_settings")).toBe(true);
    await click(q(host, "media-open-player"));
    expect(openPlayer).toHaveBeenCalled();
  });
});

describe("HUP-S10.1 media bridge validation", () => {
  it("accepts well-formed messages and drops anything else whole", () => {
    expect(parseToMainMedia({ v: 1, type: "media.ready" })).toEqual({ v: 1, type: "media.ready" });
    expect(parseToMainMedia({ v: 1, type: "media.show", id: "m1-ab" })).toEqual({ v: 1, type: "media.show", id: "m1-ab" });
    expect(parseToMainMedia({ v: 1, type: "media.save", id: "m1", grantId: "4" })).toEqual({ v: 1, type: "media.save", id: "m1", grantId: "4" });
    for (const bad of [
      null,
      { v: 2, type: "media.ready" },
      { v: 1, type: "media.show", id: "../etc" },
      { v: 1, type: "media.save", id: "m1" },
      { v: 1, type: "media.save", id: "m1", grantId: "a/b" },
      { v: 1, type: "monitor.stop" },
    ]) {
      expect(parseToMainMedia(bad)).toBeNull();
    }
  });

  it("only accepts image data URLs of PNG, JPEG or WebP", () => {
    expect(parseToMediaPopout({ v: 1, type: "media.image", id: "m1", dataUrl: PNG })).not.toBeNull();
    for (const url of ["data:image/svg+xml;base64,PHN2Zz4=", "https://x/y.png", "data:text/html;base64,PGI+", "javascript:alert(1)"]) {
      expect(parseToMediaPopout({ v: 1, type: "media.image", id: "m1", dataUrl: url })).toBeNull();
    }
    const card = toCard(item());
    expect(parseToMediaPopout({ v: 1, type: "media.gallery", items: [card], targets: [{ grantId: "4", root: "/x" }], note: null })).not.toBeNull();
    expect(parseToMediaPopout({ v: 1, type: "media.gallery", items: [{ ...card, id: "<b>" }], targets: [], note: null })).toBeNull();
    expect(parseToMediaPopout({ v: 1, type: "media.result", ok: true, message: "Saved" })).not.toBeNull();
  });
});

describe("HUP-S10.1 media host and player", () => {
  it("the host answers ready with the gallery, shows an image on request, and saves a copy", async () => {
    const b = bus();
    const { io, calls } = makeIo((cmd) =>
      cmd === "media_gallery" ? [item(), item({ id: "m2", present: false })] : cmd === "media_options" ? options() : cmd === "media_read" ? PNG : cmd === "media_save_copy" ? "/Users/m/Pictures/copy.png" : null,
    );
    const host = await createMediaHost({ transport: b.transport("main"), invoke: io.invoke });
    const got: ToMediaPopout[] = [];
    const end = await createMediaPopoutEnd(b.transport("popout-media"), (m) => got.push(m));
    await end.ready();
    await vi.waitFor(() => expect(got.some((m) => m.type === "media.gallery")).toBe(true));
    const g = got.find((m) => m.type === "media.gallery")!;
    expect(g.type === "media.gallery" && g.items.map((i) => i.present)).toEqual([true, false]);
    expect(g.type === "media.gallery" && g.targets).toEqual([{ grantId: "4", root: "/Users/m/Pictures" }]);
    await end.show("m1790812800-ab12cd34");
    await vi.waitFor(() => expect(got.some((m) => m.type === "media.image")).toBe(true));
    await end.save("m1790812800-ab12cd34", "4");
    await vi.waitFor(() => expect(got.some((m) => m.type === "media.result" && m.ok)).toBe(true));
    expect(calls.find(([c]) => c === "media_save_copy")![1]).toEqual({ id: "m1790812800-ab12cd34", grantId: "4" });
    host.dispose();
    end.close();
  });

  it("a refused save comes back as an honest failure", async () => {
    const b = bus();
    const { io } = makeIo((cmd) => (cmd === "media_save_copy" ? new Error("that folder is not granted for writing") : cmd === "media_gallery" ? [] : options()));
    await createMediaHost({ transport: b.transport("main"), invoke: io.invoke });
    const got: ToMediaPopout[] = [];
    const end = await createMediaPopoutEnd(b.transport("popout-media"), (m) => got.push(m));
    await end.save("m1", "9");
    await vi.waitFor(() => expect(got.some((m) => m.type === "media.result")).toBe(true));
    const r = got.find((m) => m.type === "media.result")!;
    expect(r.type === "media.result" && r.ok).toBe(false);
    expect(r.type === "media.result" && r.message).toContain("not granted");
  });

  it("the player shows the newest image, its cost, and saves a copy to a granted folder", async () => {
    const b = bus();
    const { io } = makeIo((cmd) =>
      cmd === "media_gallery" ? [item({ usage: { total_tokens: 50 } })] : cmd === "media_options" ? options() : cmd === "media_read" ? PNG : cmd === "media_save_copy" ? "/Users/m/Pictures/copy.png" : null,
    );
    await createMediaHost({ transport: b.transport("main"), invoke: io.invoke });
    const { host } = await mount(<PopoutRoot kind="media" transport={async () => b.transport("unused")} mediaTransport={async () => b.transport("popout-media")} />);
    await vi.waitFor(() => expect(q(host, "media-image")).toBeTruthy());
    expect(q(host, "media-image")!.getAttribute("src")).toBe(PNG);
    expect(q(host, "media-cost")!.textContent).toContain("No charge");
    expect(q(host, "media-cost")!.textContent).toContain("50 tokens");
    await click(q(host, "media-save"));
    await vi.waitFor(() => expect(q(host, "media-result")?.textContent).toContain("Saved a copy"));
  });

  it("the player says when nothing has been made, and how to save when no folder is granted", async () => {
    const b = bus();
    const { io } = makeIo((cmd) => (cmd === "media_gallery" ? [] : options({ targets: [] })));
    await createMediaHost({ transport: b.transport("main"), invoke: io.invoke });
    const { host } = await mount(<MediaPlayer transport={async () => b.transport("popout-media")} />);
    await vi.waitFor(() => expect(q(host, "media-empty")).toBeTruthy());

    const b2 = bus();
    const io2 = makeIo((cmd) => (cmd === "media_gallery" ? [item()] : cmd === "media_read" ? PNG : options({ targets: [] })));
    await createMediaHost({ transport: b2.transport("main"), invoke: io2.io.invoke });
    const m = await mount(<MediaPlayer transport={async () => b2.transport("popout-media")} />);
    await vi.waitFor(() => expect(q(m.host, "media-no-targets")).toBeTruthy());
  });
});
