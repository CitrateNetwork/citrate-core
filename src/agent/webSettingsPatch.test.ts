// The one writer of the saved web settings (patchWebSettings, startSettingsWrite.ts). Core's
// hermes_web_settings_set replaces the WHOLE object, so two read-modify-writes that overlap can put
// back a field the other just changed: a browser opt-out undone by a search toggle. These tests run
// the real writer, lock and start hold against a core stand-in whose reads and writes answer on a
// later turn, as IPC does, and fire the writes without awaiting each in turn.
//
// The tripwire at the bottom fails if any module other than the writer sends the whole object.
import { beforeEach, describe, expect, it } from "vitest";
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative } from "node:path";
import { DEFAULT_WEB_SETTINGS, type HermesWebSettings, type HermesWebStatus, type WebSettingsIo } from "./webSearch";
import { patchWebSettings } from "./startSettingsWrite";
import { subscribeWebSettings } from "./webSettingsSync";
import { hermesRunningSlice, withHermesStart } from "../shell/slices/hermesRunning";

const later = () => new Promise((r) => setTimeout(r, 0));

let saved: HermesWebSettings = DEFAULT_WEB_SETTINGS;
let failNextSet = false;
const status = (settings: HermesWebSettings): HermesWebStatus => ({
  settings,
  managedChromium: null,
  searxngFound: true,
  jinaKeyFileFound: false,
  jevKeyFileFound: false,
  notices: [],
  appliesOnRestart: true,
});
const io: WebSettingsIo = {
  mode: "tauri",
  invoke: (async (cmd: string, args?: Record<string, unknown>) => {
    if (cmd === "hermes_web_settings_get") {
      const snapshot = saved; // what core has when the read is made
      await later();
      return status(snapshot);
    }
    if (cmd === "hermes_web_settings_set") {
      await later();
      if (failNextSet) {
        failNextSet = false;
        throw new Error("core refused the write");
      }
      saved = (args as { settings: HermesWebSettings }).settings; // replaces the whole object
      return status(saved);
    }
    throw new Error(`not served: ${cmd}`);
  }) as WebSettingsIo["invoke"],
};

const form = Symbol("form");
const toggle = Symbol("toggle");

beforeEach(() => {
  saved = { ...DEFAULT_WEB_SETTINGS, browserEnabled: true, searchEnabled: false };
  failNextSet = false;
  hermesRunningSlice.set({ webSearch: null, learn: null, starting: 0 });
});

describe("patchWebSettings: overlapping writers", () => {
  it("a browser opt-out overlapping a search toggle: both changes persist", async () => {
    await Promise.all([patchWebSettings(io, { browserEnabled: false }, form), patchWebSettings(io, { searchEnabled: true }, toggle)]);
    expect(saved.browserEnabled).toBe(false);
    expect(saved.searchEnabled).toBe(true);
  });

  it("the same overlap in the other order: both changes persist", async () => {
    await Promise.all([patchWebSettings(io, { searchEnabled: true }, toggle), patchWebSettings(io, { browserEnabled: false }, form)]);
    expect(saved).toEqual({ ...DEFAULT_WEB_SETTINGS, browserEnabled: false, searchEnabled: true });
  });

  it("three overlapping writers each keep their field, and the last answer is what core holds", async () => {
    const answers = await Promise.all([
      patchWebSettings(io, { browserEnabled: false }, form),
      patchWebSettings(io, { searchEnabled: true }, toggle),
      patchWebSettings(io, { jevEnabled: true }, form),
    ]);
    expect(saved).toEqual({ ...DEFAULT_WEB_SETTINGS, browserEnabled: false, searchEnabled: true, jevEnabled: true });
    expect(answers[2]?.settings).toEqual(saved);
  });

  it("a refused write rejects to its caller, changes nothing, and the next write still lands", async () => {
    failNextSet = true;
    const first = patchWebSettings(io, { browserEnabled: false }, form);
    const second = patchWebSettings(io, { searchEnabled: true }, toggle);
    await expect(first).rejects.toThrow("core refused the write");
    await second;
    expect(saved).toEqual({ ...DEFAULT_WEB_SETTINGS, browserEnabled: true, searchEnabled: true });
  });

  it("other views are told what core saved, tagged with the writer", async () => {
    const seen: [boolean, symbol][] = [];
    const off = subscribeWebSettings((st, source) => seen.push([st.settings.browserEnabled, source]));
    await patchWebSettings(io, { browserEnabled: false }, form);
    off();
    expect(seen).toEqual([[false, form]]);
  });

  it("a write asked for while Hermes is starting is held: null, and core is unchanged", async () => {
    let release: () => void = () => {};
    const start = withHermesStart(
      () => new Promise<void>((r) => (release = r)),
      async () => ({ webSearch: saved.searchEnabled, learn: null }),
    );
    await later();
    const held = await patchWebSettings(io, { browserEnabled: false }, form);
    release();
    await start;
    expect(held).toBeNull();
    expect(saved.browserEnabled).toBe(true);
  });
});

// Tripwire: hermes_web_settings_set replaces the whole object, so only the one writer may send it.
describe("tripwire: no component writes the whole web-settings object directly", () => {
  const SRC = join(__dirname, "..");
  const files: string[] = [];
  const walk = (dir: string) => {
    for (const n of readdirSync(dir)) {
      const p = join(dir, n);
      if (statSync(p).isDirectory()) walk(p);
      else if (/\.(ts|tsx)$/.test(n) && !/\.test\.(ts|tsx)$/.test(n)) files.push(p);
    }
  };
  walk(SRC);
  const rel = (p: string) => relative(SRC, p).split("\\").join("/");

  it("only agent/webSearch.ts sends the hermes_web_settings_set command", () => {
    const hits = files.filter((p) => /["'`]hermes_web_settings_set["'`]/.test(readFileSync(p, "utf8"))).map(rel);
    expect(hits).toEqual(["agent/webSearch.ts"]);
  });

  it("only agent/startSettingsWrite.ts calls saveWebSettings", () => {
    const hits = files
      .filter((p) => rel(p) !== "agent/webSearch.ts")
      .filter((p) => /\bsaveWebSettings\s*\(/.test(readFileSync(p, "utf8")))
      .map(rel);
    expect(hits).toEqual(["agent/startSettingsWrite.ts"]);
  });
});
