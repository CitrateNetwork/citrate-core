// =====================================================================
// citrate-core: the authority an FL action runs under (FL design board 11, HIC-2 envelope)
//
// HIC-1 by default: the member approves every action. HIC-2 only while an envelope the member
// signed is on, named with what is left of its caps. The envelope lives in core storage (board 11,
// Settings › Work); until that read lands, and whenever there is no envelope or it is off, this
// says HIC-1, which is the truth on 0.5.4: no envelope can be signed in this build.
//
// `selectAuthority()` is synchronous and reads a module cache; `useAuthority()` re-renders on
// change. Only Settings › Work writes the cache (setEnvelopeForAuthority).
// =====================================================================
import { useSyncExternalStore } from "react";

export type Authority = { level: "HIC-1" } | { level: "HIC-2"; name: string; caps: { label: string; left: string }[] };

export const HIC1: Authority = { level: "HIC-1" };

/** What Settings › Work hands the cache: the signed envelope, or null when there is none or it is off. */
export interface EnvelopeView {
  on: boolean;
  name: string;
  caps: { label: string; left: string }[];
}

let envelope: EnvelopeView | null = null;
let current: Authority = HIC1;
const listeners = new Set<() => void>();

function derive(e: EnvelopeView | null): Authority {
  if (!e || !e.on) return HIC1;
  return { level: "HIC-2", name: e.name, caps: e.caps.map((c) => ({ ...c })) };
}

export function selectAuthority(): Authority {
  return current;
}

export function subscribeAuthority(fn: () => void): () => void {
  listeners.add(fn);
  return () => {
    listeners.delete(fn);
  };
}

/** Settings › Work only: the envelope as core read it. Turning it off applies at once. */
export function setEnvelopeForAuthority(e: EnvelopeView | null): void {
  envelope = e ? { ...e, caps: e.caps.map((c) => ({ ...c })) } : null;
  current = derive(envelope);
  for (const l of [...listeners]) l();
}

export function useAuthority(): Authority {
  return useSyncExternalStore(subscribeAuthority, selectAuthority, selectAuthority);
}
