// CONNECT-S0 — the People directory aggregation (pure, testable).
//
// A "person" in your directory is anyone you share a group with (keyed on their comms address — the
// identity the rosters key on), shown by their verified face (@handle) where one resolves, listing the
// groups you have in common and their role in each. This module is PURE: it takes already-fetched
// groups + rosters + resolved faces + your own address and derives the directory. No fabrication — a
// person with no verified face keeps a null face (rendered as a short address), and no groups → no rows
// (Rule 1). The store wires it to the live `groups`/`social` bridge reads; the surface renders it.

export interface PersonFace {
  network: string;
  handle: string;
}
export interface PersonGroup {
  id: string;
  name: string;
  role: string;
}
export interface Person {
  /** The person's comms address (canonical identity, verbatim from the roster). */
  address: string;
  /** A verified, group-visible face if one resolves for this address, else null. */
  face: PersonFace | null;
  /** The groups you share with this person + their role in each (deduped). */
  groups: PersonGroup[];
}

/**
 * Derive the People directory from live data. `rosterByGroup[groupId]` is that group's `(address, role)`
 * roster. `faces` are resolved (verified, group-visible) identities. `selfAddr` (your comms address) is
 * excluded so you never appear in your own directory. Deterministic order: people with a face first
 * (recognizable), then by number of shared groups (desc), then address — so the most-connected, most-
 * identifiable people surface at the top.
 */
export function buildPeopleDirectory(
  groups: { id: string; name: string }[],
  rosterByGroup: Record<string, { address: string; role: string }[]>,
  faces: { address: string; network: string; handle: string }[],
  selfAddr: string,
): Person[] {
  const self = (selfAddr || "").toLowerCase();
  const faceByAddr = new Map<string, PersonFace>();
  for (const f of faces) faceByAddr.set(f.address.toLowerCase(), { network: f.network, handle: f.handle });

  const byAddr = new Map<string, Person>();
  for (const g of groups) {
    const roster = rosterByGroup[g.id] || [];
    for (const m of roster) {
      const key = m.address.toLowerCase();
      if (!key || key === self) continue;
      let p = byAddr.get(key);
      if (!p) {
        p = { address: m.address, face: faceByAddr.get(key) ?? null, groups: [] };
        byAddr.set(key, p);
      }
      if (!p.groups.some((x) => x.id === g.id)) {
        p.groups.push({ id: g.id, name: g.name, role: m.role });
      }
    }
  }

  return [...byAddr.values()].sort((a, b) => {
    if (!!a.face !== !!b.face) return a.face ? -1 : 1;
    if (a.groups.length !== b.groups.length) return b.groups.length - a.groups.length;
    return a.address.toLowerCase().localeCompare(b.address.toLowerCase());
  });
}

/** Client-side filter over the derived directory: matches a person by @handle, address, or a shared
 *  group name. Empty query returns the list unchanged. */
export function filterPeople(people: Person[], query: string): Person[] {
  const q = query.trim().toLowerCase();
  if (!q) return people;
  return people.filter(
    (p) =>
      p.address.toLowerCase().includes(q) ||
      (p.face ? p.face.handle.toLowerCase().includes(q) : false) ||
      p.groups.some((g) => g.name.toLowerCase().includes(q)),
  );
}
