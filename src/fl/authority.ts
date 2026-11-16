// =====================================================================
// citrate-core: the authority an FL action runs under (FL design board 11, HIC-2 envelope)
//
// HIC-1 by default: the member approves every action. HIC-2 only while an envelope the member
// approved is on, named with what is left of its caps. Data source (Rule 7): core's fl_work.rs,
// `<app data>/fl-work/envelope.sealed` (fl_work_envelope_get). Until that read answers, and
// whenever there is no envelope or it is off, this says HIC-1.
//
// `selectAuthority()` is synchronous and reads a module cache; `useAuthority()` re-renders on
// change. The first subscriber reads the envelope from core; Settings › Work updates the cache
// after approve and off (setEnvelopeForAuthority).
// =====================================================================
import { useSyncExternalStore } from "react";
import { BRIDGE_MODE } from "../bridge/mode";
import { invoke } from "../bridge/tauri/invoke";

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

let hydrated = false;

export function subscribeAuthority(fn: () => void): () => void {
  if (!hydrated) {
    hydrated = true;
    void readEnvelope().then(
      (e) => setEnvelopeForAuthority(envelopeView(e)),
      () => {
        /* unread: stays HIC-1, the safe default */
      },
    );
  }
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

// ------------------------------------------------------------------ core's envelope (fl_work.rs)

/** The envelope as core stores it. */
export interface EnvelopeRecord {
  on: boolean;
  name: string;
  joinsPerWeek: number;
  maxHoursPerRound: number;
  approvedAt: number;
  offAt: number | null;
  weekStart: number;
  joinsThisWeek: number;
}

export interface EnvelopeInput {
  name: string;
  joinsPerWeek: number;
  maxHoursPerRound: number;
}

export const ENVELOPE_NEEDS_DESKTOP = "The envelope is kept by the desktop app";

/** What the composer and cards show for an envelope. */
export function envelopeView(e: EnvelopeRecord | null): EnvelopeView | null {
  if (!e) return null;
  const left = Math.max(0, e.joinsPerWeek - e.joinsThisWeek);
  return {
    on: e.on,
    name: e.name,
    caps: [
      { label: "joins this week", left: `${left} of ${e.joinsPerWeek}` },
      { label: "machine hours per round", left: `up to ${e.maxHoursPerRound}` },
    ],
  };
}

export function readEnvelope(): Promise<EnvelopeRecord | null> {
  if (BRIDGE_MODE !== "tauri") return Promise.reject(new Error(ENVELOPE_NEEDS_DESKTOP));
  return invoke<EnvelopeRecord | null>("fl_work_envelope_get");
}

/** Seal an envelope the member approved on their card, and switch authority to it. */
export async function approveEnvelope(input: EnvelopeInput): Promise<EnvelopeRecord> {
  if (BRIDGE_MODE !== "tauri") throw new Error(ENVELOPE_NEEDS_DESKTOP);
  const e = await invoke<EnvelopeRecord>("fl_work_envelope_approve", { envelope: input });
  setEnvelopeForAuthority(envelopeView(e));
  return e;
}

/** Turn the envelope off now (lowering authority needs no card). */
export async function envelopeOff(): Promise<EnvelopeRecord | null> {
  setEnvelopeForAuthority(null);
  if (BRIDGE_MODE !== "tauri") throw new Error(ENVELOPE_NEEDS_DESKTOP);
  const e = await invoke<EnvelopeRecord | null>("fl_work_envelope_off");
  setEnvelopeForAuthority(envelopeView(e));
  return e;
}
