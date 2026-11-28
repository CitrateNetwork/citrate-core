// L0-WP06: the slice registry. New 0.6.0 lane state goes into a slice registered here, never into
// AppState. The registry refuses what would make two lanes collide (an id, a storage key), keeps
// persisted slices inside the citrate-core.v60.* namespace, and refuses webview storage for a
// sealed slice (D16: sealed state persists through the L9 sealed store only).
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createSlice } from "./createSlice";
import { registerSlice, registeredSlices, type SliceRegistration } from "./registry";
import { currentNetKey } from "../netScope";

// The registry is process-wide (one per app), so every test uses ids and keys of its own.
let n = 0;
const uniq = (name: string) => `${name}${++n}`;

beforeEach(() => localStorage.clear());
afterEach(() => localStorage.clear());

describe("slice registry", () => {
  it("register_rejects_duplicate_ids", () => {
    const id = uniq("l4.groupHistory");
    const first = createSlice({ items: [] as string[] });
    expect(registerSlice({ id, lane: "L4", slice: first })).toBe(first);
    expect(() => registerSlice({ id, lane: "L4", slice: createSlice({ items: [] as string[] }) })).toThrow(/already registered/);
    expect(registeredSlices().filter((r) => r.id === id)).toHaveLength(1);
    expect(registeredSlices().find((r) => r.id === id)?.slice).toBe(first);
  });

  it("register_rejects_an_id_outside_its_lane", () => {
    expect(() => registerSlice({ id: uniq("l3.checklist"), lane: "L4", slice: createSlice({ a: 1 }) })).toThrow(/must start with "l4\."/);
    expect(() => registerSlice({ id: "l4.", lane: "L4", slice: createSlice({ a: 1 }) })).toThrow(/must start with "l4\."/);
    expect(() => registerSlice({ id: uniq("l4.x"), lane: "L10" as SliceRegistration<object>["lane"], slice: createSlice({ a: 1 }) })).toThrow(/lane/);
  });

  it("register_rejects_storage_key_outside_v60_prefix", () => {
    const bad = ["citrate-core-proto-v2", "citrate.groups.history.v1", "citrate-core.v60.", "citrate-core.v60.a@b", "citrate-core.v60.A", "xcitrate-core.v60.a", "citrate-core.v60.a b"];
    for (const storageKey of bad) {
      expect(
        () =>
          registerSlice({
            id: uniq("l4.k"),
            lane: "L4",
            slice: createSlice({ a: 1 }),
            persist: { scope: "device", keys: ["a"], storageKey: storageKey as `citrate-core.v60.${string}` },
          }),
        storageKey,
      ).toThrow(/storage key/);
    }
    expect(localStorage.length).toBe(0);
  });

  it("sealed_slice_cannot_use_localstorage", () => {
    const id = uniq("l4.groupHistory");
    const storageKey = `citrate-core.v60.${uniq("sealed")}` as const;
    localStorage.setItem(storageKey, JSON.stringify({ items: ["from webview storage"] }));
    const slice = createSlice({ items: [] as string[] });
    expect(() => registerSlice({ id, lane: "L4", slice, sealed: true, persist: { scope: "device", keys: ["items"], storageKey } })).toThrow(
      "sealed slices persist through the sealed store only",
    );
    // Refused before anything was read or registered.
    expect(slice.get().items).toEqual([]);
    expect(registeredSlices().some((r) => r.id === id)).toBe(false);
    // Fail closed: any value other than false or absent counts as sealed.
    const truthy = "yes" as unknown as boolean;
    expect(() => registerSlice({ id: uniq("l4.g"), lane: "L4", slice: createSlice({ items: [] as string[] }), sealed: truthy, persist: { scope: "device", keys: ["items"], storageKey } })).toThrow(/sealed/);
    // A sealed slice without a persist block lives in memory only, and is registered.
    const memOnly = createSlice({ items: [] as string[] });
    expect(registerSlice({ id: uniq("l4.mem"), lane: "L4", slice: memOnly, sealed: true })).toBe(memOnly);
    memOnly.set({ items: ["x"] });
    expect(localStorage.getItem(storageKey)).toBe(JSON.stringify({ items: ["from webview storage"] }));
    expect(localStorage.length).toBe(1);
  });

  it("persisted_slice_round_trips", () => {
    const storageKey = `citrate-core.v60.${uniq("checklist")}` as const;
    const fresh = () => createSlice({ done: false, order: [] as string[], busy: false });
    const a = fresh();
    registerSlice({ id: uniq("l3.checklist"), lane: "L3", slice: a, persist: { scope: "device", keys: ["done", "order"], storageKey } });
    a.set({ done: true, order: ["s1", "s2"], busy: true });
    // Only the listed keys are written, on every change.
    const saved = localStorage.getItem(storageKey) as string;
    expect(JSON.parse(saved)).toEqual({ done: true, order: ["s1", "s2"] });
    // The next launch reads what this one wrote. One process cannot register a key twice, so the
    // save is carried to a second key, as a relaunch would find it.
    const nextKey = `citrate-core.v60.${uniq("checklist")}` as const;
    localStorage.setItem(nextKey, saved);
    const b = fresh();
    registerSlice({ id: uniq("l3.checklist"), lane: "L3", slice: b, persist: { scope: "device", keys: ["done", "order"], storageKey: nextKey } });
    // Listed keys come back; an unlisted key keeps its default.
    expect(b.get()).toEqual({ done: true, order: ["s1", "s2"], busy: false });
    // Restoring writes nothing back by itself.
    expect(localStorage.getItem(nextKey)).toBe(saved);
  });

  it("lanes_cannot_share_a_storage_key", () => {
    const storageKey = `citrate-core.v60.${uniq("shared")}` as const;
    registerSlice({ id: uniq("l4.a"), lane: "L4", slice: createSlice({ a: 1 }), persist: { scope: "device", keys: ["a"], storageKey } });
    expect(() => registerSlice({ id: uniq("l3.a"), lane: "L3", slice: createSlice({ a: 1 }), persist: { scope: "device", keys: ["a"], storageKey } })).toThrow(/storage key .* is already used by l4\./);
    // The same lane cannot reuse it either: one key, one slice.
    expect(() => registerSlice({ id: uniq("l4.b"), lane: "L4", slice: createSlice({ a: 1 }), persist: { scope: "device", keys: ["a"], storageKey } })).toThrow(/already used/);
  });

  it("network_scoped_slice_is_kept_per_genesis", () => {
    const storageKey = `citrate-core.v60.${uniq("net")}` as const;
    const net = currentNetKey();
    const a = createSlice({ stake: "0" });
    registerSlice({ id: uniq("l8.round"), lane: "L8", slice: a, persist: { scope: "network", keys: ["stake"], storageKey } });
    a.set({ stake: "5" });
    // Written under this genesis only, never under the bare key another genesis would read.
    expect(localStorage.getItem(storageKey)).toBeNull();
    expect(JSON.parse(localStorage.getItem(`${storageKey}@${net}`) as string)).toEqual({ stake: "5" });
    // A value saved under another genesis is never restored here.
    const other = `citrate-core.v60.${uniq("net")}` as const;
    localStorage.setItem(`${other}@0x${"ee".repeat(32)}`, JSON.stringify({ stake: "9" }));
    localStorage.setItem(other, JSON.stringify({ stake: "9" }));
    const b = createSlice({ stake: "0" });
    registerSlice({ id: uniq("l8.round"), lane: "L8", slice: b, persist: { scope: "network", keys: ["stake"], storageKey: other } });
    expect(b.get().stake).toBe("0");
  });

  it("persist_keys_must_be_fields_of_the_slice", () => {
    const storageKey = `citrate-core.v60.${uniq("keys")}` as const;
    const slice = createSlice({ a: 1 });
    const bad = { scope: "device" as const, keys: ["b"] as unknown as "a"[], storageKey };
    expect(() => registerSlice({ id: uniq("l5.k"), lane: "L5", slice, persist: bad })).toThrow(/"b" is not a field/);
    expect(() => registerSlice({ id: uniq("l5.k"), lane: "L5", slice, persist: { scope: "device", keys: [], storageKey } })).toThrow(/at least one key/);
    expect(() => registerSlice({ id: uniq("l5.k"), lane: "L5", slice, persist: { scope: "everywhere" as "device", keys: ["a"], storageKey } })).toThrow(/scope/);
  });

  it("a_saved_value_of_another_kind_is_not_restored", () => {
    const storageKey = `citrate-core.v60.${uniq("kind")}` as const;
    localStorage.setItem(storageKey, JSON.stringify({ name: "Dana", n: "5", list: { 0: "a" } }));
    const slice = createSlice({ name: null as string | null, n: 0, list: [] as string[] });
    registerSlice({ id: uniq("l3.kind"), lane: "L3", slice, persist: { scope: "device", keys: ["name", "n", "list"], storageKey } });
    // A null default takes a saved primitive; a number never takes a string, an array never an object.
    expect(slice.get()).toEqual({ name: "Dana", n: 0, list: [] });
    const other = `citrate-core.v60.${uniq("kind")}` as const;
    localStorage.setItem(other, JSON.stringify({ name: { injected: true } }));
    const t = createSlice({ name: null as string | null });
    registerSlice({ id: uniq("l3.kind"), lane: "L3", slice: t, persist: { scope: "device", keys: ["name"], storageKey: other } });
    expect(t.get().name).toBeNull();
  });

  it("a_damaged_save_never_throws_and_never_restores_a_key_it_does_not_list (generated saves)", () => {
    // Property check over generated saved values: junk blobs, wrong types, extra keys.
    let x = 20261127;
    const rand = () => ((x = (Math.imul(x, 1664525) + 1013904223) >>> 0) / 2 ** 32);
    const junk: unknown[] = [null, 0, -1, "", "x", true, [], {}, [1, "a"], { nested: 1 }, "not json", "{"];
    const initial = { count: 0, label: "none", tags: [] as string[], extra: "kept" };
    for (let i = 0; i < 200; i++) {
      const storageKey = `citrate-core.v60.${uniq("prop")}` as const;
      let raw: string;
      if (i % 10 === 0) raw = String(junk[Math.floor(rand() * junk.length)]);
      else {
        const blob: Record<string, unknown> = {};
        for (const k of ["count", "label", "tags", "extra", "__proto__", "constructor"]) {
          if (rand() < 0.5) continue;
          const value = rand() < 0.5 ? junk[Math.floor(rand() * junk.length)] : (initial as Record<string, unknown>)[k] ?? 1;
          // defineProperty, so "__proto__" is an own key of the blob as JSON.parse would make it.
          Object.defineProperty(blob, k, { value, enumerable: true, configurable: true, writable: true });
        }
        raw = JSON.stringify(blob);
      }
      localStorage.setItem(storageKey, raw);
      const slice = createSlice({ ...initial, tags: [] as string[] });
      expect(() => registerSlice({ id: uniq("l6.p"), lane: "L6", slice, persist: { scope: "device", keys: ["count", "label", "tags"], storageKey } }), `case ${i}`).not.toThrow();
      const s = slice.get() as Record<string, unknown>;
      expect(s.extra, `case ${i}`).toBe("kept");
      expect(Object.keys(s).sort(), `case ${i}`).toEqual(["count", "extra", "label", "tags"]);
      expect(typeof s.count, `case ${i}`).toBe("number");
      expect(typeof s.label, `case ${i}`).toBe("string");
      expect(Array.isArray(s.tags), `case ${i}`).toBe(true);
      expect(Object.getPrototypeOf(s), `case ${i}`).toBe(Object.prototype);
    }
  });
});
