// CX bridge impl — groups (C-19), SIM. Owned by lane s3 (CX-S3).
//
// The dev shim: a FUNCTIONAL in-memory groups store so the Groups surface behaves in web-dev the
// same way it will over the real daemon. It starts EMPTY (Rule 1 / the frozen contract test: no
// fabricated rooms or messages) and fills only as the user creates groups + sends — never seeded.
// Guarded out of packaged builds by the bridge mode. secp addresses are a clearly-fake sim self.
import type { Group, GroupMessage, GroupRole, GroupsDomain } from "../domains";
import type { SimHost } from "./index";

/** The dev user's address in sim (clearly fake, valid hex) — the owner/sender of sim groups. */
const SIM_SELF = "0x5100000000000000000000000000000000000051";

interface SimGroup extends Group {
  msgs: GroupMessage[];
}

export function simGroups(_host: SimHost): GroupsDomain {
  const groups = new Map<string, SimGroup>();
  let seq = 1;

  const view = (g: SimGroup): Group => ({
    id: g.id,
    owner: g.owner,
    kind: g.kind,
    members: g.members.map((m) => ({ ...m })),
  });

  return {
    async create(kind, _name): Promise<Group> {
      const id = `sim-g${seq++}`;
      const g: SimGroup = {
        id,
        owner: SIM_SELF,
        kind,
        members: [{ address: SIM_SELF, role: "owner" }],
        msgs: [],
      };
      groups.set(id, g);
      return view(g);
    },
    async list() {
      return [...groups.values()].map(view);
    },
    async join() {
      /* sim: single dev user; join is a no-op */
    },
    async roster(groupId) {
      return groups.get(groupId)?.members.map((m) => ({ ...m })) ?? [];
    },
    async assignRole(groupId, address, role: GroupRole) {
      const g = groups.get(groupId);
      if (!g) return;
      const m = g.members.find((x) => x.address === address);
      if (m) m.role = role;
      else g.members.push({ address, role });
    },
    async offboard(groupId, address) {
      const g = groups.get(groupId);
      if (g) g.members = g.members.filter((m) => m.address !== address);
    },
    async send(groupId, body) {
      const g = groups.get(groupId);
      if (g) g.msgs.push({ id: `${groupId}:${g.msgs.length}`, groupId, sender: SIM_SELF, body, ts: 0 });
    },
    async messages(groupId): Promise<GroupMessage[]> {
      return groups.get(groupId)?.msgs.map((m) => ({ ...m })) ?? [];
    },
  };
}
