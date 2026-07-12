import { SurfaceProps, StubShell } from "./shared";

export function Wallet(_: SurfaceProps) {
  return <StubShell title="Wallet" eyebrow={"source · smart wallet"} note="Overview, Staking, Activity, and Identity tabs — balances, send/receive, add/withdraw stake, the client-side signature ledger, and the member SBT panel." />;
}
