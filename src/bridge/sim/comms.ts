// CX bridge impl — groups (C-19), SIM. Owned by lane s3 after S0. Honest-empty (Rule 1).
import type { GroupsDomain, Group } from "../domains";
import type { SimHost } from "./index";

export function simGroups(_host: SimHost): GroupsDomain {
  return {
    async create(kind): Promise<Group> {
      return { id: "sim-group", owner: "0x", kind, members: [] };
    },
    async list() {
      return [];
    },
    async join() {
      /* sim: no-op */
    },
    async roster() {
      return [];
    },
    async assignRole() {
      /* sim: no-op */
    },
    async offboard() {
      /* sim: no-op */
    },
    async send() {
      /* sim: no relay */
    },
    async messages() {
      return [];
    },
  };
}
