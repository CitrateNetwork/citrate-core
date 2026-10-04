// HUP-S8.2/S8.3 (US-8.1) — the fleet wizard's state machine. Written red-first.
import { describe, it, expect } from "vitest";
import {
  initialWizard,
  reduce,
  deviceRows,
  needsConnectivityHelp,
  roleLabel,
  expiresIn,
  canBrowse,
  type WizardState,
} from "./wizard";
import type { FleetDevice, FleetProbe, JoinResult, PairOffer, SeenDevice, TailscaleView } from "../bridge/tauri/fleet";
import { sampleReport } from "../shell/slices/tierTestReport";

const probe: FleetProbe = {
  device: { deviceId: "a".repeat(32), label: "This Mac", tier: "T2", role: "heavy" },
  tier: sampleReport(),
};

const seen: SeenDevice = {
  advert: { id: "abcd", label: "Linux box", tier: "T1", role: "worker", port: 0 },
  ip: "192.168.1.30",
  ageSecs: 2,
};

const paired: FleetDevice = {
  id: "b".repeat(32),
  label: "Old laptop",
  tier: "T0",
  role: "light",
  addr: "192.168.1.40",
  pairedAt: 100,
  via: "issued",
};

const offer: PairOffer = {
  link: "citrate://pair?c=x&s=y",
  qr: { size: 2, rows: ["10", "01"] },
  expiresAt: 1_000 + 600,
  hints: ["192.168.1.20:41234"],
};

function run(...actions: Parameters<typeof reduce>[1][]): WizardState {
  return actions.reduce(reduce, initialWizard());
}

describe("fleet wizard: steps", () => {
  it("starts at the intro, with discovery OFF and no consent given", () => {
    const s = initialWizard();
    expect(s.step).toBe("intro");
    expect(s.discovery.enabled).toBe(false);
    expect(canBrowse(s)).toBe(false);
  });

  it("probing moves to the machine step and records this device", () => {
    const s = run({ type: "start" }, { type: "probed", probe });
    expect(s.step).toBe("machine");
    expect(s.probe?.device.role).toBe("heavy");
    expect(s.busy).toBe(false);
  });

  it("start marks busy and clears an old error", () => {
    const s = run({ type: "failed", error: "boom" }, { type: "start" });
    expect(s.busy).toBe(true);
    expect(s.error).toBeNull();
  });

  it("goto moves between steps", () => {
    const s = run({ type: "probed", probe }, { type: "goto", step: "pair" });
    expect(s.step).toBe("pair");
  });

  it("a failure is shown and stops the busy state", () => {
    const s = run({ type: "start" }, { type: "failed", error: "probe failed" });
    expect(s.error).toBe("probe failed");
    expect(s.busy).toBe(false);
  });
});

describe("fleet wizard: discovery is opt-in", () => {
  it("browsing is only possible after the member turns discovery on", () => {
    let s = run({ type: "probed", probe }, { type: "goto", step: "discover" });
    expect(canBrowse(s)).toBe(false);
    s = reduce(s, { type: "discoverySet", enabled: true });
    expect(canBrowse(s)).toBe(true);
  });

  it("turning discovery off forgets what was found", () => {
    const s = run({ type: "discoverySet", enabled: true }, { type: "browsed", devices: [seen] }, { type: "discoverySet", enabled: false });
    expect(s.discovery.devices).toEqual([]);
    expect(s.discovery.browsed).toBe(false);
  });

  it("browse results are ignored if discovery was turned off meanwhile", () => {
    const s = run({ type: "browsed", devices: [seen] });
    expect(s.discovery.devices).toEqual([]);
  });
});

describe("fleet wizard: pairing", () => {
  it("a created link is kept with its QR", () => {
    const s = run({ type: "offer", offer });
    expect(s.offer?.qr.size).toBe(2);
  });

  it("a successful join adds the device and clears unreachable", () => {
    const ok: JoinResult = { ok: true, device: { ...paired, via: "joined" }, errorKind: null, message: null, tried: [] };
    const s = run({ type: "joined", result: { ...ok, ok: false, errorKind: "unreachable", device: null, message: "x", tried: ["a"] } }, { type: "joined", result: ok });
    expect(s.unreachable).toBe(false);
    expect(s.roster.map((d) => d.id)).toEqual([paired.id]);
    expect(s.step).toBe("done");
  });

  it("an unreachable join sends the member to the connectivity step", () => {
    const bad: JoinResult = { ok: false, device: null, errorKind: "unreachable", message: "could not reach", tried: ["192.168.1.20:41234"] };
    const s = run({ type: "goto", step: "pair" }, { type: "joined", result: bad });
    expect(s.unreachable).toBe(true);
    expect(s.step).toBe("connect");
    expect(s.error).toBe("could not reach");
  });

  it("a refused join stays on the pair step with the reason", () => {
    const bad: JoinResult = { ok: false, device: null, errorKind: "refused", message: "That pairing link was already used; create a new one.", tried: [] };
    const s = run({ type: "goto", step: "pair" }, { type: "joined", result: bad });
    expect(s.step).toBe("pair");
    expect(s.unreachable).toBe(false);
    expect(s.error).toContain("already used");
  });

  it("editing the link clears the previous check", () => {
    const s = run(
      { type: "inspected", claim: { v: 1, nonce: "n", issuerPub: "p", issuerLabel: "Studio", issuerTier: "T2", issuedAt: 1, expiresAt: 2, hints: [] } },
      { type: "linkInput", link: "citrate://pair?c=z" },
    );
    expect(s.inspect).toBeNull();
    expect(s.joinLink).toBe("citrate://pair?c=z");
  });
});

describe("fleet wizard: device list shows tier and role", () => {
  it("lists this machine, paired machines, then machines seen on the network", () => {
    const s = run(
      { type: "probed", probe },
      { type: "roster", devices: [paired] },
      { type: "discoverySet", enabled: true },
      { type: "browsed", devices: [seen] },
    );
    const rows = deviceRows(s);
    expect(rows.map((r) => r.where)).toEqual(["this machine", "paired", "on this network"]);
    expect(rows.map((r) => r.tier)).toEqual(["T2", "T0", "T1"]);
    expect(rows.map((r) => r.role)).toEqual(["heavy", "light", "worker"]);
    expect(rows[2].addr).toBe("192.168.1.30");
  });

  it("role labels are plain language and unknown stays unknown", () => {
    expect(roleLabel("heavy")).toMatch(/larger models/);
    expect(roleLabel("light")).toMatch(/chat/);
    expect(roleLabel("worker")).toMatch(/jobs/);
    expect(roleLabel("unknown")).toBe("Role unknown");
  });
});

describe("fleet wizard: connectivity help (S8.3)", () => {
  it("is not needed when a machine was found or paired", () => {
    const s = run({ type: "discoverySet", enabled: true }, { type: "browsed", devices: [seen] });
    expect(needsConnectivityHelp(s)).toBe(false);
  });

  it("is needed when a browse found nothing and nothing is paired", () => {
    const s = run({ type: "discoverySet", enabled: true }, { type: "browsed", devices: [] });
    expect(needsConnectivityHelp(s)).toBe(true);
  });

  it("is needed after an unreachable join", () => {
    const s = run({ type: "joined", result: { ok: false, device: null, errorKind: "unreachable", message: "x", tried: [] } });
    expect(needsConnectivityHelp(s)).toBe(true);
  });

  it("keeps the tailscale view", () => {
    const ts: TailscaleView = { report: { state: "notInstalled", version: null, selfHost: null, selfIps: [], peers: [] }, guidance: [{ id: "install", text: "Install", url: "https://tailscale.com/download" }] };
    const s = run({ type: "tailscale", view: ts });
    expect(s.tailscale?.guidance[0].id).toBe("install");
  });
});

describe("expiresIn", () => {
  it("counts down in minutes, then says expired", () => {
    expect(expiresIn(1_600, 1_000)).toBe("expires in 10 min");
    expect(expiresIn(1_600, 1_570)).toBe("expires in under a minute");
    expect(expiresIn(1_600, 1_600)).toBe("expired");
  });
});

// HUP-S8.1/S8.2 follow-on: DeviceLinks travel with the pairing; a pairing link can arrive by deep link.
import { linkNote } from "./wizard";

describe("fleet wizard: device links and deep links", () => {
  it("records whether this machine is linked", () => {
    const s = run({ type: "probed", probe }, { type: "linkStatus", status: { linked: true, label: "Studio Mac" } });
    expect(s.link).toEqual({ linked: true, label: "Studio Mac" });
  });

  it("a deep-linked pairing link lands on the pair step, filled in, without pairing", () => {
    const s = run({ type: "probed", probe }, { type: "prefill", link: "citrate://pair?c=a&s=b" });
    expect(s.step).toBe("pair");
    expect(s.joinLink).toBe("citrate://pair?c=a&s=b");
    expect(s.roster).toEqual([]);
  });

  it("shows, per paired machine, what happened to its device link", () => {
    const added = { ...paired, id: "c".repeat(32), deviceLink: "added" as const };
    const other = { ...paired, id: "d".repeat(32), deviceLink: "otherMember" as const };
    const s = run({ type: "probed", probe }, { type: "roster", devices: [added, other, paired] });
    const rows = deviceRows(s).filter((r) => r.where === "paired");
    expect(rows.map((r) => r.link)).toEqual(["added", "otherMember", null]);
    expect(linkNote("added")).toMatch(/linked under you/);
    expect(linkNote("otherMember")).toMatch(/another person/);
    expect(linkNote("refused")).toMatch(/did not verify/);
    expect(linkNote(null)).toMatch(/not linked/);
  });
});
