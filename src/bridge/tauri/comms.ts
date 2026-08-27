// CX bridge impl — groups (C-19), TAURI. Owned by lane s3 (CX-S3) after S0.
// S0.2 stub: honest Unavailable. CX-S3 wires the comms-core relay + Group roster/RBAC.
import type { GroupsDomain } from "../domains";
import { Unavailable } from "../types";

export const tauriGroups: GroupsDomain = {
  async create() {
    throw new Unavailable("groups", "create");
  },
  async list() {
    throw new Unavailable("groups", "list");
  },
  async join() {
    throw new Unavailable("groups", "join");
  },
  async roster() {
    throw new Unavailable("groups", "roster");
  },
  async assignRole() {
    throw new Unavailable("groups", "assignRole");
  },
  async offboard() {
    throw new Unavailable("groups", "offboard");
  },
  async send() {
    throw new Unavailable("groups", "send");
  },
  async messages() {
    throw new Unavailable("groups", "messages");
  },
};
