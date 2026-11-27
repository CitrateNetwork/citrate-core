// 0.6.0 L3-WP00 characterization: Citrate Core has no OS notifications today (citrate-core
// a826e441), pinned before L3-WP13 (Tauri notification plugin and permission flow) and L3-WP12
// (delete Comms) change it. A failure here means notifications were added or the Comms copy
// changed: update the expectation only together with that reviewed change.
//
// The Comms surface says "New mentions fire an OS notification", which nothing in the app does.
// That false copy is pinned ON PURPOSE, so WP12 or WP13 has to flip this expectation deliberately.
import { describe, it, expect } from "vitest";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";

const read = (p: string) => readFileSync(p, "utf8");

function sources(dir: string, ext: RegExp): string[] {
  const out: string[] = [];
  for (const name of readdirSync(dir)) {
    const p = join(dir, name);
    if (statSync(p).isDirectory()) out.push(...sources(p, ext));
    else if (ext.test(name) && !/\.test\.(ts|tsx)$/.test(name) && !/_tests?\.rs$/.test(name)) out.push(p);
  }
  return out;
}

describe("L3-WP00 characterization: no OS notifications", () => {
  it("no_notification_plugin_in_cargo_or_capabilities", () => {
    // No notification plugin crate, and tauri has no features (so no tray-icon either).
    const cargo = read("src-tauri/Cargo.toml");
    expect(cargo).not.toContain("tauri-plugin-notification");
    expect(cargo).toContain('tauri = { version = "2", features = [] }');
    expect(read("Cargo.lock")).not.toContain('name = "tauri-plugin-notification"');
    // No JS notification plugin.
    const pkg = JSON.parse(read("package.json")) as { dependencies?: Record<string, string>; devDependencies?: Record<string, string> };
    const deps = Object.keys({ ...pkg.dependencies, ...pkg.devDependencies });
    expect(deps).not.toContain("@tauri-apps/plugin-notification");
    // No capability grants a notification permission.
    for (const f of readdirSync("src-tauri/capabilities").filter((n) => n.endsWith(".json"))) {
      expect(read(join("src-tauri/capabilities", f)), f).not.toMatch(/notification/i);
    }
    // No Rust code registers the plugin or a tray, and no webview code posts a notification.
    for (const p of sources("src-tauri/src", /\.rs$/)) {
      const s = read(p);
      expect(s, p).not.toContain("tauri_plugin_notification");
      expect(s, p).not.toContain("TrayIconBuilder");
    }
    for (const p of sources("src", /\.(ts|tsx)$/)) {
      const s = read(p);
      expect(s, p).not.toContain("plugin-notification");
      expect(s, p).not.toMatch(/new Notification\(|Notification\.requestPermission/);
    }
  });

  it("comms_copy_claims_os_notification", () => {
    const comms = read("src/surfaces/Comms.tsx").replace(/\s+/g, " ");
    expect(comms).toContain("New mentions fire an OS notification.");
    expect(comms).toContain("this center polls every 20 seconds, honestly.");
  });
});
