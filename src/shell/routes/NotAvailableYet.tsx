// The honest state for a tab whose content is not in this build (Rule 1: never placeholder data).
// Moved verbatim from PlaceView.tsx (L0-WP08), which re-exports it, so a lane's route file
// (src/shell/routes/l<N>.tsx) can render it without importing PlaceView.tsx, which loads that file.

/** The honest state for a tab whose content is not in this build. */
export function NotAvailableYet({ route, title, body, action }: { route: string; title: string; body: string; action?: { label: string; go: () => void } }) {
  return (
    <div data-not-available={route} style={{ padding: "20px 26px 24px", display: "flex", flexDirection: "column", gap: 16, boxSizing: "border-box" }}>
      <span style={{ fontFamily: "var(--font-display)", fontWeight: 420, fontSize: 24 }}>{title}</span>
      <div className="surface" style={{ padding: "18px 20px", display: "flex", flexDirection: "column", gap: 10, maxWidth: 620 }}>
        <span className="eyebrow">Not available yet</span>
        <p style={{ fontSize: 13, lineHeight: 1.6, color: "var(--tx-2)", margin: 0 }}>{body}</p>
        {action && (
          <span>
            <button className="btn btn-ghost btn-sm" onClick={action.go}>
              {action.label}
            </button>
          </span>
        )}
      </div>
    </div>
  );
}
