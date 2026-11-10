// K6-3 (K6e): Settings › Network shows the network this build targets, read from the network
// profile (src/network/profile.ts → src-tauri/network/profiles.json), and an honest
// "Update required" for both a retired network and a profile that is not set.
import { describe, it, expect } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { NetworkProfilePanel } from "./NetworkPanel";
import { ACTIVE_PROFILE, PROFILES, type NetworkProfile } from "./profile";
import type { NetworkCompat } from "../shell/networkCompat";

const active = (): NetworkProfile => ({ ...ACTIVE_PROFILE!, pending: { ...ACTIVE_PROFILE!.pending } });
const unsetProfile = (): NetworkProfile => PROFILES.find((p) => p.id === "genesis-2026-12")!;

function render(profile: NetworkProfile | null, status: NetworkCompat, live: string | null = null): string {
  return renderToStaticMarkup(<NetworkProfilePanel profile={profile} status={status} live={live} />);
}

describe("Settings › Network reads the profile (K6-3)", () => {
  it("renders the network name, chain id, RPC, explorer and every bootnode from the profile", () => {
    const p = active();
    const html = render(p, "compatible");
    expect(html).toContain(p.label);
    expect(html).toContain(String(p.chainId));
    expect(html).toContain(p.rpcUrl!);
    expect(html).toContain(p.explorerUrl!);
    for (const b of p.bootnodes!) {
      const hostPort = b.slice(b.indexOf("@") + 1);
      expect(html).toContain(hostPort);
    }
    expect(html).not.toContain("boot-eu1");
  });

  it("follows the profile: another profile's values render, none of the active one's", () => {
    const p: NetworkProfile = {
      ...active(),
      label: "Rehearsal network",
      chainId: 41001,
      rpcUrl: "https://rpc.rehearsal.example.org",
      explorerUrl: "https://explorer.rehearsal.example.org",
      bootnodes: ["noise_" + "ab".repeat(32) + "@boot9.rehearsal.example.org:30303"],
    };
    const html = render(p, "compatible");
    expect(html).toContain("Rehearsal network");
    expect(html).toContain("41001");
    expect(html).toContain("https://rpc.rehearsal.example.org");
    expect(html).toContain("https://explorer.rehearsal.example.org");
    expect(html).toContain("boot9.rehearsal.example.org:30303");
    expect(html).not.toContain(ACTIVE_PROFILE!.rpcUrl!);
    expect(html).not.toContain("boot1.citrate.ai");
  });

  it("an unset profile shows each missing value with what sets it, never a placeholder", () => {
    const p = unsetProfile();
    const html = render(p, "unset");
    expect(html).toContain(p.label);
    expect(html).toContain("not set");
    expect(html).toContain(p.pending.rpcUrl);
    expect(html).toContain(p.pending.bootnodes);
    expect(html).toContain(p.pending.chainId);
    expect(html).not.toContain("rpc.citrate.ai");
    expect(html).not.toContain("boot1.citrate.ai");
  });

  it("a ready, matching network does not say Update required", () => {
    const html = render(active(), "compatible");
    expect(html).not.toContain("Update required");
    expect(html).toContain("live network matches");
  });

  it("before block 0 is read the state is unknown, not matching and not retired", () => {
    const html = render(active(), "unknown");
    expect(html).not.toContain("Update required");
    expect(html).not.toContain("live network matches");
    expect(html).toContain("checking");
  });
});

describe('"Update required" covers both the retired and the unset profile (K6-3)', () => {
  it("retired: the live block 0 differs from this build's genesis", () => {
    const other = "0x98e0d72f422049606a6b29ca0a9bcfd2300753fd36526d2c8e9f9a2531b70c73";
    const html = render(active(), "retired", other);
    expect(html).toContain("Update required");
    expect(html).toContain("different genesis");
    expect(html).toContain(other.slice(0, 10));
  });

  it("unset: this build has no network set", () => {
    const html = render(unsetProfile(), "unset");
    expect(html).toContain("Update required");
    expect(html).toContain("no network set");
  });

  it("unset: a ready-looking profile with one value missing still says Update required", () => {
    const html = render({ ...active(), addressBook: null }, "unset");
    expect(html).toContain("Update required");
    expect(html).toContain("addressBook not set");
  });

  it("no active profile at all says Update required", () => {
    const html = render(null, "unset");
    expect(html).toContain("Update required");
    expect(html).toContain("no active network profile");
  });
});
