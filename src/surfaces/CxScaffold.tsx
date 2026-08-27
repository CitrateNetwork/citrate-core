// CX scaffold placeholder (CX-S0.4). An honest empty state (Rule 1): the surface exists in
// nav + routing so its lane can fill it in its OWN file, but nothing is fabricated until then.
export function CxScaffold(props: { title: string; sprint: string; detail: string }) {
  return (
    <div style={{ padding: 40, color: "var(--tx-2)", maxWidth: 560 }}>
      <h1 style={{ fontSize: 22, color: "var(--tx-1)", marginBottom: 8 }}>{props.title}</h1>
      <p style={{ fontSize: 14, lineHeight: 1.5 }}>{props.detail}</p>
      <p style={{ fontSize: 12.5, marginTop: 18, color: "var(--tx-3)" }}>
        Lands in <strong>{props.sprint}</strong> (planset citrate-core-social). Scaffolded — this
        surface fabricates no data (Rule 1).
      </p>
    </div>
  );
}
