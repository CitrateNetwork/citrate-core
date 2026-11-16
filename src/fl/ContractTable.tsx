// =====================================================================
// citrate-core: the FL contract table (FL design board 05, Today)
//
// One row per FL contract with what this network says about it, in the required words, and its
// source. Everything comes from the network state model (networkState.ts).
// =====================================================================
import { contractSource, contractWords, HUB_CONTRACTS, type FlContractName, type FlNetworkState } from "./networkState";
import { Hex, SourceTag, Unknown } from "./primitives";

export function ContractTable({ state, names = HUB_CONTRACTS }: { state: FlNetworkState; names?: FlContractName[] }) {
  return (
    <table className="fl-tbl" data-testid="fl-contract-table" style={{ maxWidth: 760 }}>
      <thead>
        <tr>
          <th style={{ width: "30%" }}>Contract</th>
          <th style={{ width: "48%" }}>On this network</th>
          <th>Source</th>
        </tr>
      </thead>
      <tbody>
        {names.map((n) => {
          const c = state.contracts[n];
          const w = contractWords(c);
          return (
            <tr key={n} data-contract={n} data-kind={c.kind}>
              <td>{n}</td>
              <td>
                {w.unknown && w.text === "—" ? <Unknown reason={w.unknown} /> : w.text}
                {w.unknown && w.text !== "—" && <span className="fl-faint"> ({w.unknown})</span>}
                {"address" in c && c.kind !== "reading" && (
                  <>
                    {" "}
                    <Hex value={c.address} />
                  </>
                )}
              </td>
              <td>
                <SourceTag>{contractSource(n, c)}</SourceTag>
              </td>
            </tr>
          );
        })}
      </tbody>
    </table>
  );
}
