// K6-3 red stub: keeps the old hard-coded bootnode line so the new tests compile and fail.
import type { NetworkProfile } from "./profile";
import type { NetworkCompat } from "../shell/networkCompat";

export function NetworkProfilePanel(_: { profile: NetworkProfile | null; status: NetworkCompat; live: string | null }) {
  return <span className="mono">boot1.citrate.ai:30303 · boot2.citrate.ai:30303 · boot-eu1.citrate.ai:30303</span>;
}
