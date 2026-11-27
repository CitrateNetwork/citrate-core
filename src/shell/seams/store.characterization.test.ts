// L0-WP02 SEAM-STORE: the Store's public surface, recorded at a826e441. L0-WP07 moves action bodies
// into src/shell/actions/l<N>/ and keeps each method as a one-line delegate, so every name, kind and
// arity here must stay byte-identical. A changed fixture needs an L0 reviewer and a reason in the PR.
import { describe, expect, it } from "vitest";
import { Store } from "../store";
import { SEAMS_BASE, checkSnapshot, diffFixture } from "../../../scripts/seams-snapshot.mjs";

type Kind = string;

/** Each own member of `o`: `method/<arity>`, `get`, `set`, `get+set`, or the value's type. */
function members(o: object, skip: string[] = []): Record<string, Kind> {
  const out: Record<string, Kind> = {};
  for (const name of Object.getOwnPropertyNames(o).sort()) {
    if (skip.includes(name)) continue;
    const d = Object.getOwnPropertyDescriptor(o, name);
    if (!d) continue;
    if (typeof d.value === "function") out[name] = `method/${d.value.length}`;
    else if (d.get || d.set) out[name] = [d.get && "get", d.set && "set"].filter(Boolean).join("+");
    else out[name] = typeof d.value;
  }
  return out;
}

function storeSurface() {
  return {
    prototype: members(Store.prototype),
    statics: members(Store, ["length", "name", "prototype"]),
    constructorArity: Store.length,
  };
}

const FIXTURE = `fixtures/seams/store-methods.${SEAMS_BASE}.json`;

describe("SEAM-STORE: Store public methods", () => {
  it("store_public_methods_are_unchanged", () => {
    const surface = storeSurface();
    // A parse that found almost nothing would make the snapshot vacuous.
    expect(Object.keys(surface.prototype).length).toBeGreaterThan(200);
    expect(checkSnapshot(FIXTURE, surface)).toEqual([]);
  });

  it("mutation: a dropped method or a changed arity is named", () => {
    const dropped = storeSurface();
    delete dropped.prototype.save;
    expect(diffFixture(FIXTURE, dropped)).toEqual(["- prototype.save"]);
    const arity = storeSurface();
    arity.prototype.go = "method/9";
    expect(diffFixture(FIXTURE, arity)).toEqual([`~ prototype.go: "${storeSurface().prototype.go}" -> "method/9"`]);
  });
});
