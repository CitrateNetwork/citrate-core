// A placeholder for a surface that is on the roadmap but not yet interactive. It shows the real
// description + an honest "in development" note in plain, user-facing language — no internal sprint
// codes or engineering jargon, and it never fabricates data. `sprint` is kept for call-site
// compatibility but is not shown to the user.
export function CxScaffold(props: { title: string; sprint?: string; detail: string }) {
  return (
    <div style={{ padding: 40, color: "var(--tx-2)", maxWidth: 560 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 10, marginBottom: 8 }}>
        <h1 style={{ fontSize: 22, color: "var(--tx-1)", margin: 0 }}>{props.title}</h1>
        <span
          style={{
            fontSize: 11,
            fontWeight: 600,
            letterSpacing: ".04em",
            textTransform: "uppercase",
            color: "var(--tx-3)",
            border: "1px solid var(--line-1)",
            borderRadius: 999,
            padding: "2px 9px",
          }}
        >
          In development
        </span>
      </div>
      <p style={{ fontSize: 14, lineHeight: 1.5 }}>{props.detail}</p>
      <p style={{ fontSize: 12.5, marginTop: 18, color: "var(--tx-3)" }}>
        This part of Citrate is on the way and will light up in an update. Nothing here is
        placeholder data — when it’s ready, what you see is real.
      </p>
    </div>
  );
}
