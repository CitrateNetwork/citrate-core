// HUP-S5.1 + S5.6 — the Browser view's data: the sidecar's status and screencast frames, checked
// before they reach the pop-out. A frame's pixels must be plain base64 JPEG; anything malformed is
// dropped whole; an unknown status reads as "off", never as a working browser.
import { describe, it, expect } from "vitest";
import { parseBrowserStatus, parseBrowserFrame, parseBrowserView, frameSrc, highlightBox, BROWSER_OFF } from "./browserView";

const status = {
  enabled: true,
  chromium: { state: "system", path: "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" },
  mode: "attached",
  attachPort: 9222,
  stopped: false,
  url: "https://docs.example.org/a",
  consentedOrigins: ["https://docs.example.org"],
  excludedCategories: [
    { id: "banking", label: "Banking, payments and exchanges" },
    { id: "email", label: "Email" },
    { id: "health", label: "Health records and insurance" },
  ],
  consentNeeded: { origin: "https://www.chase.com", category: "banking" },
  pendingAction: { id: "b3", tool: "browser_act", summary: 'Click [e2] button "Continue" on https://docs.example.org/a', reason: "untrusted" },
};

const frame = {
  version: 12,
  mime: "image/jpeg",
  data: "/9j/4AAQSkZJRg==",
  viewportWidth: 1280,
  viewportHeight: 800,
  url: "https://docs.example.org/a",
  withheld: false,
  highlight: { ref: "e2", label: 'button "Continue"', x: 640, y: 200, width: 128, height: 40, state: "pending" },
};

describe("HUP-S5.1 browser status", () => {
  it("parses the sidecar's status", () => {
    const s = parseBrowserStatus(status);
    expect(s.enabled).toBe(true);
    expect(s.mode).toBe("attached");
    expect(s.attachPort).toBe(9222);
    expect(s.chromium).toEqual({ state: "system", path: status.chromium.path, searched: null });
    expect(s.consentNeeded).toEqual({ origin: "https://www.chase.com", category: "banking" });
    expect(s.pendingAction?.id).toBe("b3");
    expect(s.excludedCategories.map((c) => c.id)).toEqual(["banking", "email", "health"]);
  });

  it("reads anything unknown as off", () => {
    for (const raw of [null, undefined, 3, "x", {}, { enabled: "yes" }, { enabled: false }]) {
      expect(parseBrowserStatus(raw)).toEqual(BROWSER_OFF);
    }
    expect(parseBrowserStatus({ enabled: false, running: false }).running).toBe(false);
  });

  it("says not installed with how many places were checked", () => {
    const s = parseBrowserStatus({ ...status, chromium: { state: "not_installed", searched: ["/a", "/b"] } });
    expect(s.chromium).toEqual({ state: "not_installed", path: null, searched: 2 });
  });

  it("drops malformed parts instead of trusting them", () => {
    const s = parseBrowserStatus({ ...status, mode: "root", consentedOrigins: ["https://ok.example", 7], pendingAction: { id: 3 } });
    expect(s.mode).toBe("off");
    expect(s.consentedOrigins).toEqual(["https://ok.example"]);
    expect(s.pendingAction).toBeNull();
  });
});

describe("HUP-S5.1 screencast frames", () => {
  it("parses a frame and builds a data URL from base64 only", () => {
    const f = parseBrowserFrame(frame);
    expect(f?.version).toBe(12);
    expect(frameSrc(f)).toBe("data:image/jpeg;base64,/9j/4AAQSkZJRg==");
  });

  it("refuses frames that are not plain base64 JPEG", () => {
    for (const bad of [
      { ...frame, data: "abc\"><script>" },
      { ...frame, data: "ab cd" },
      { ...frame, mime: "text/html" },
      { ...frame, version: -1 },
      { ...frame, version: "12" },
      { ...frame, viewportWidth: Number.NaN },
      { ...frame, data: "A".repeat(16 * 1024 * 1024 + 4) },
    ]) {
      expect(parseBrowserFrame(bad)).toBeNull();
    }
    expect(parseBrowserFrame(null)).toBeNull();
  });

  it("a withheld frame has no picture", () => {
    const f = parseBrowserFrame({ ...frame, withheld: true, data: "" });
    expect(f?.withheld).toBe(true);
    expect(frameSrc(f)).toBeNull();
    expect(frameSrc(parseBrowserFrame({ ...frame, data: "" }))).toBeNull();
  });

  it("places the element outline as a share of the viewport", () => {
    const f = parseBrowserFrame(frame);
    expect(highlightBox(f)).toEqual({ left: "50%", top: "25%", width: "10%", height: "5%", label: '[e2] button "Continue"', pending: true });
    expect(highlightBox(parseBrowserFrame({ ...frame, highlight: null }))).toBeNull();
    expect(highlightBox(parseBrowserFrame({ ...frame, viewportWidth: 0 }))).toBeNull();
    // A malformed outline is dropped, the frame kept.
    const odd = parseBrowserFrame({ ...frame, highlight: { ref: "e2", x: "1" } });
    expect(odd?.highlight).toBeNull();
  });
});

describe("HUP-S5.1 the bridge view", () => {
  it("round-trips a view and rejects a broken one", () => {
    const v = parseBrowserView({ state: parseBrowserStatus(status), frame: parseBrowserFrame(frame) });
    expect(v?.state.mode).toBe("attached");
    expect(v?.frame?.version).toBe(12);
    expect(parseBrowserView({ state: parseBrowserStatus(status), frame: null })?.frame).toBeNull();
    expect(parseBrowserView({ state: parseBrowserStatus(status), frame: { ...frame, data: "<x>" } })).toBeNull();
    expect(parseBrowserView({ frame: null })).toBeNull();
    expect(parseBrowserView(null)).toBeNull();
  });
});

describe("HUP-S2.3 sign-in requests in the status", () => {
  it("keeps well-formed requests and drops the rest", () => {
    const s = parseBrowserStatus({
      ...status,
      mode: "managed",
      signInRequests: [
        { id: "signin-1-5", kind: "personal_sign", raiseOrigin: "https://app.example.org", topFrame: true, messageHex: "6869" },
        { id: "signin-2-5", kind: "accounts", raiseOrigin: "https://app.example.org", topFrame: false },
        { id: "nope", kind: "accounts", raiseOrigin: "x", topFrame: true },
        { id: "signin-3-5", kind: "eth_sign", raiseOrigin: "x", topFrame: true },
        7,
      ],
    });
    expect(s.signInRequests).toEqual([
      { id: "signin-1-5", kind: "personal_sign", raiseOrigin: "https://app.example.org", topFrame: true },
      { id: "signin-2-5", kind: "accounts", raiseOrigin: "https://app.example.org", topFrame: false },
    ]);
    expect(parseBrowserStatus(status).signInRequests).toEqual([]);
    expect(BROWSER_OFF.signInRequests).toEqual([]);
  });
});
