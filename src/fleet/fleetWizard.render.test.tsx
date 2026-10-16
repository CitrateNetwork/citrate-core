// HUP-S8.2/S8.3 (US-8.1) — the fleet wizard view, one static render per state. Written red-first.
import { describe, it, expect } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { FleetWizardView, type FleetWizardHandlers } from "./FleetWizard";
import { initialWizard, reduce, type WizardState, type WizardAction } from "./wizard";
import type { FleetProbe, PairOffer, TailscaleView } from "../bridge/tauri/fleet";
import { sampleReport } from "../shell/slices/tierTestReport";

const noop = () => {};
const h: FleetWizardHandlers = {
  onStart: noop,
  onGoto: noop,
  onDiscovery: noop,
  onBrowse: noop,
  onCreateLink: noop,
  onLinkInput: noop,
  onInspect: noop,
  onJoin: noop,
  onRename: noop,
};

const probe: FleetProbe = {
  device: { deviceId: "a".repeat(32), label: "This Mac", tier: "T2", role: "heavy" },
  tier: sampleReport(),
};

const offer: PairOffer = {
  link: "citrate://pair?c=abc&s=def",
  qr: { size: 3, rows: ["101", "010", "111"] },
  expiresAt: 1_600,
  hints: ["192.168.1.20:41234", "100.64.1.10:41234"],
};

function st(...a: WizardAction[]): WizardState {
  return a.reduce(reduce, initialWizard());
}

const render = (s: WizardState, available = true) =>
  renderToStaticMarkup(<FleetWizardView state={s} nowSecs={1_000} available={available} {...h} />);

describe("FleetWizardView", () => {
  it("intro offers to connect machines", () => {
    const html = render(initialWizard());
    expect(html).toContain("Connect my machines");
    expect(html).toContain('data-testid="fleet-start"');
  });

  it("outside the desktop app it says so and offers nothing", () => {
    const html = render(initialWizard(), false);
    expect(html).toContain("available in the desktop app");
    expect(html).not.toContain('data-testid="fleet-start"');
  });

  it("machine step shows this machine's tier, role and rationale", () => {
    const html = render(st({ type: "probed", probe }));
    expect(html).toContain("This Mac");
    expect(html).toContain("T2");
    expect(html).toContain("Heavy: serves the larger models");
    expect(html).toContain("24 GB or more usable");
    expect(html).toContain("pending owner sign-off");
  });

  // HUP F-10 / WP-S8.2: once the staged patch lands, the tier and role come from the citrate-sizeup
  // library (`library::recommend` + `Role::for_tier`); the machine step must render every outcome it returns,
  // including the dedicated-GPU lift and the unknown-memory T0, with the rationale lines tier.rs emits.
  it.each([
    { tier: "T0", role: "light", label: "Light: chat and small jobs", line: "Under 12 GB usable → T0" },
    { tier: "T1", role: "worker", label: "Worker: mid-size models and background jobs", line: "12–24 GB usable → T1" },
    { tier: "T2", role: "heavy", label: "Heavy: serves the larger models", line: "16 GB or more of dedicated GPU memory → T2" },
    {
      tier: "T0",
      role: "light",
      label: "Light: chat and small jobs",
      line: "Memory size could not be read on this machine, so the smallest tier is used to stay safe",
    },
  ] as const)("machine step renders the sizeup outcome $tier/$role: $line", ({ tier, role, label, line }) => {
    const report = sampleReport({ effective: tier });
    report.recommendation = { ...report.recommendation, tier, rationale: [line], guided: tier === "T0" };
    const p: FleetProbe = { device: { ...probe.device, label: "This PC", tier, role }, tier: report };
    const html = render(st({ type: "probed", probe: p }));
    expect(html).toContain(tier);
    expect(html).toContain(label);
    expect(html).toContain(line);
    expect(html).toContain("pending owner sign-off");
  });

  it("machine step without a tier says the tier and role are unknown, never a guess", () => {
    const p: FleetProbe = { device: { ...probe.device, tier: null, role: "unknown" }, tier: sampleReport() };
    const html = render(st({ type: "probed", probe: p }));
    expect(html).toContain("tier unknown");
    expect(html).toContain("Role unknown");
  });

  it("discovery is off by default, browsing disabled, and says what would be shared", () => {
    const html = render(st({ type: "probed", probe }, { type: "goto", step: "discover" }));
    expect(html).toContain('data-testid="fleet-discovery-toggle"');
    expect(html).not.toMatch(/data-testid="fleet-discovery-toggle"[^>]*checked/);
    expect(html).toMatch(/data-testid="fleet-browse"[^>]*disabled/);
    expect(html).toContain("off until you turn it on");
    expect(html).toContain("No wallet address");
  });

  it("discovered machines are listed with tier and role", () => {
    const html = render(
      st(
        { type: "probed", probe },
        { type: "goto", step: "discover" },
        { type: "discoverySet", enabled: true },
        { type: "browsed", devices: [{ advert: { id: "abcd", label: "Linux box", tier: "T1", role: "worker", port: 0 }, ip: "192.168.1.30", ageSecs: 1 }] },
      ),
    );
    expect(html).toMatch(/data-testid="fleet-discovery-toggle"[^>]*checked/);
    expect(html).toContain("Linux box");
    expect(html).toContain("on this network");
    expect(html).toContain("Worker: mid-size models");
  });

  it("an empty browse says nothing was found and points at connectivity help", () => {
    const html = render(st({ type: "goto", step: "discover" }, { type: "discoverySet", enabled: true }, { type: "browsed", devices: [] }));
    expect(html).toContain("No other Citrate Core machines answered");
    expect(html).toContain('data-testid="fleet-goto-connect"');
  });

  it("pair step shows the link, a QR of it, its expiry, and that it works once", () => {
    const html = render(st({ type: "probed", probe }, { type: "goto", step: "pair" }, { type: "offer", offer }));
    expect(html).toContain('data-testid="fleet-qr"');
    expect((html.match(/<rect /g) ?? []).length).toBe(6 + 1); // dark modules + background
    expect(html).toContain("citrate://pair?c=abc&amp;s=def");
    expect(html).toContain("expires in 10 min");
    expect(html).toContain("works once");
    expect(html).toContain("100.64.1.10:41234");
  });

  it("a checked link names the machine it came from", () => {
    const html = render(
      st(
        { type: "goto", step: "pair" },
        { type: "linkInput", link: "citrate://pair?c=x&s=y" },
        { type: "inspected", claim: { v: 1, nonce: "n", issuerPub: "p", issuerLabel: "Studio", issuerTier: "T2", issuedAt: 900, expiresAt: 1_500, hints: [] } },
      ),
    );
    expect(html).toContain("Signed link from Studio (T2)");
    expect(html).toContain('data-testid="fleet-join"');
  });

  it("connect step shows read-only Tailscale status and guidance", () => {
    const view: TailscaleView = {
      report: { state: "notInstalled", version: null, selfHost: null, selfIps: [], peers: [] },
      guidance: [{ id: "install", text: "Install Tailscale on each machine.", url: "https://tailscale.com/download" }],
    };
    const html = render(
      st({ type: "joined", result: { ok: false, device: null, errorKind: "unreachable", message: "The other machine could not be reached at any of its addresses.", tried: ["192.168.1.20:41234"] } }, { type: "tailscale", view }),
    );
    expect(html).toContain("Tailscale is not installed");
    expect(html).toContain("Install Tailscale on each machine.");
    expect(html).toContain('href="https://tailscale.com/download"');
    expect(html).toContain("never changes your Tailscale settings");
    expect(html).toContain("could not be reached");
  });

  it("connect step lists tailnet machines with online state", () => {
    const view: TailscaleView = {
      report: {
        state: "running",
        version: "1.90",
        selfHost: "studio",
        selfIps: ["100.64.1.10"],
        peers: [{ hostName: "linux-box", os: "linux", ips: ["100.64.1.11"], online: true }],
      },
      guidance: [],
    };
    const html = render(st({ type: "goto", step: "connect" }, { type: "tailscale", view }));
    expect(html).toContain("Tailscale is connected");
    expect(html).toContain("100.64.1.10");
    expect(html).toContain("linux-box");
  });

  it("done step lists devices and says, per machine, whether its device link came across", () => {
    const html = render(
      st(
        { type: "probed", probe },
        { type: "joined", result: { ok: true, device: { id: "b".repeat(32), label: "Studio", tier: "T2", role: "heavy", addr: "192.168.1.20", pairedAt: 1, via: "joined", deviceLink: "added" }, errorKind: null, message: null, tried: [] } },
      ),
    );
    expect(html).toContain("Studio");
    expect(html).toContain("paired");
    expect(html).toContain("linked under you");
    expect(html).toContain("wallet-signed device link");
    expect(html).toContain("Groups");
    const unlinked = render(
      st(
        { type: "probed", probe },
        { type: "joined", result: { ok: true, device: { id: "c".repeat(32), label: "Old box", tier: null, role: "unknown", addr: null, pairedAt: 1, via: "joined" }, errorKind: null, message: null, tried: [] } },
      ),
    );
    expect(unlinked).toContain("not linked yet");
  });

  it("pair step offers to link this machine first, and an install link for a machine without Citrate Core", () => {
    const withInstall: PairOffer = {
      ...offer,
      installUrl: "https://citrate.ai/download",
      installQr: { size: 2, rows: ["11", "01"] },
      carriesDeviceLink: false,
    };
    const html = renderToStaticMarkup(
      <FleetWizardView
        state={st({ type: "probed", probe }, { type: "goto", step: "pair" }, { type: "offer", offer: withInstall }, { type: "linkStatus", status: { linked: false, label: null } })}
        nowSecs={1_000}
        available
        canLink
        {...h}
        onLinkDevice={noop}
      />,
    );
    expect(html).toContain('data-testid="fleet-link-device"');
    expect(html).toContain("not linked yet");
    expect(html).toContain('data-testid="fleet-install-qr"');
    expect(html).toContain("https://citrate.ai/download");
    const linked = render(st({ type: "probed", probe }, { type: "goto", step: "pair" }, { type: "linkStatus", status: { linked: true, label: "Studio Mac" } }));
    expect(linked).toContain("Studio Mac");
    expect(linked).not.toContain('data-testid="fleet-link-device"');
  });

  it("an error is shown as an alert", () => {
    const html = render(st({ type: "failed", error: "The fleet roster file could not be read." }));
    expect(html).toContain('role="alert"');
    expect(html).toContain("could not be read");
  });

  it("uses no em-dashes in member-facing text", () => {
    const html = [
      render(initialWizard()),
      render(st({ type: "probed", probe })),
      render(st({ type: "goto", step: "discover" })),
      render(st({ type: "goto", step: "pair" }, { type: "offer", offer })),
      render(st({ type: "goto", step: "connect" })),
      render(st({ type: "goto", step: "done" })),
    ].join("");
    expect(html).not.toContain("—");
  });
});
