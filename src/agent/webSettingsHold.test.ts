// How the web-settings write lock (webSettingsSync.ts) composes with the Hermes start hold
// (shell/slices/hermesRunning.ts). Hermes reads the saved web settings when it starts, so:
//   - a start takes its snapshot only after any web-settings write already in progress has landed
//     (the snapshot is what Hermes will read);
//   - a write still queued when a start begins does not land during the start (it is held);
//   - neither waits on the other forever: starts, nested starts and writes all settle, and writes
//     work again once the start is over.
import { beforeEach, describe, expect, it } from "vitest";
import { withWebSettingsLock } from "./webSettingsSync";
import { writeWebSettingsUnlessStarting } from "./webSettingsWrite";
import { hermesRunningSlice, hermesStarting, withHermesStart, type HermesSavedSnapshot } from "../shell/slices/hermesRunning";

const tick = () => new Promise((r) => setTimeout(r, 0));

// A stand-in for what core has saved: reads and writes answer on a later turn, as IPC does.
let saved = { searchEnabled: false };
const readSaved = async (): Promise<HermesSavedSnapshot> => {
  const v = saved.searchEnabled;
  await tick();
  return { webSearch: v, learn: null };
};
const writeSearch = (on: boolean, log: string[]) => async () => {
  log.push(`read ${on}`);
  await tick();
  saved = { ...saved, searchEnabled: on };
  log.push(`wrote ${on}`);
  return on;
};

/** Wait until the write has taken the lock and begun (its re-read is on the wire). */
async function begun(log: string[], entry: string) {
  for (let i = 0; i < 20 && !log.includes(entry); i++) await Promise.resolve();
  expect(log).toContain(entry);
}

/** Fails the test instead of hanging if `p` never settles (a deadlock). */
function settles<T>(p: Promise<T>, label: string): Promise<T> {
  return Promise.race([p, new Promise<T>((_, rej) => setTimeout(() => rej(new Error(`${label} never settled`)), 1000))]);
}

beforeEach(() => {
  saved = { searchEnabled: false };
  hermesRunningSlice.set({ webSearch: null, learn: null, starting: 0 });
});

describe("web-settings writes and the Hermes start hold", () => {
  it("a start that begins while a write is in progress snapshots the written value", async () => {
    const log: string[] = [];
    const write = writeWebSettingsUnlessStarting(writeSearch(true, log));
    await begun(log, "read true");
    const start = withHermesStart(async (snapshot) => snapshot, readSaved);
    const snapshot = await settles(start, "start");
    expect(await write).toBe(true);
    expect(snapshot.webSearch).toBe(true);
    expect(log).toEqual(["read true", "wrote true"]);
  });

  it("a write asked for in the same turn as a start, before it took the lock, is held (the start came first)", async () => {
    const log: string[] = [];
    const write = writeWebSettingsUnlessStarting(writeSearch(true, log));
    const start = withHermesStart(async (snapshot) => snapshot, readSaved);
    expect(await settles(write, "write")).toBeNull();
    expect((await settles(start, "start")).webSearch).toBe(false);
    expect(log).toEqual([]);
  });

  it("a write still queued when a start begins is held: it does not run, and the caller learns it was held", async () => {
    const log: string[] = [];
    const first = writeWebSettingsUnlessStarting(writeSearch(true, log));
    await begun(log, "read true");
    const queued = writeWebSettingsUnlessStarting(writeSearch(false, log));
    let release: () => void = () => {};
    const confirmed = new Promise<void>((r) => (release = r));
    const start = withHermesStart(async (snapshot) => {
      // Hermes is starting; the start is not confirmed yet.
      await confirmed;
      return snapshot;
    }, readSaved);
    expect(await settles(first, "first write")).toBe(true);
    expect(await settles(queued, "queued write")).toBeNull();
    expect(hermesStarting()).toBe(true);
    release();
    expect((await settles(start, "start")).webSearch).toBe(true);
    expect(log).toEqual(["read true", "wrote true"]);
    expect(saved.searchEnabled).toBe(true);
  });

  it("a write asked for during a start is held, and the same write works once the start is over", async () => {
    const log: string[] = [];
    let release: () => void = () => {};
    const confirmed = new Promise<void>((r) => (release = r));
    const start = withHermesStart(() => confirmed, readSaved);
    await tick();
    expect(await settles(writeWebSettingsUnlessStarting(writeSearch(true, log)), "held write")).toBeNull();
    release();
    await settles(start, "start");
    expect(hermesStarting()).toBe(false);
    expect(await settles(writeWebSettingsUnlessStarting(writeSearch(true, log)), "write after start")).toBe(true);
    expect(saved.searchEnabled).toBe(true);
  });

  it("nested starts (a restart inside a start, as the Controls restart does) and queued writes all settle", async () => {
    const log: string[] = [];
    const w1 = writeWebSettingsUnlessStarting(writeSearch(true, log));
    await begun(log, "read true");
    const outer = withHermesStart(
      (outerSnap) => withHermesStart(async (innerSnap) => [outerSnap.webSearch, innerSnap.webSearch], readSaved),
      readSaved,
    );
    const w2 = writeWebSettingsUnlessStarting(writeSearch(false, log));
    expect(await settles(outer, "nested start")).toEqual([true, true]);
    expect(await settles(w1, "w1")).toBe(true);
    expect(await settles(w2, "w2")).toBeNull();
    expect(hermesStarting()).toBe(false);
    // The lock is free again.
    expect(await settles(withWebSettingsLock(async () => "free"), "lock after starts")).toBe("free");
  });

  it("a failed snapshot read ends the start and frees the lock", async () => {
    const failing = withHermesStart(async () => "ran", async () => {
      throw new Error("core went away");
    });
    await expect(settles(failing, "failing start")).rejects.toThrow("core went away");
    expect(hermesStarting()).toBe(false);
    expect(await settles(writeWebSettingsUnlessStarting(async () => "ok"), "write after failed start")).toBe("ok");
  });
});
