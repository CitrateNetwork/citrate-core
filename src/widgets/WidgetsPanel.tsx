// =====================================================================
// citrate-core — Widgets card on the Hermes home: your widgets + the gallery (HUP-S10.3)
//
// Saved widgets render as sandboxed tiles (WidgetFrame). The gallery adds a starter template in one
// click; Hermes can also propose one in chat (the `widget_create` tool, which always asks first).
// Each tile says who wrote it and which data it can read. Desktop app only.
// =====================================================================
import { useEffect, useState } from "react";
import { WidgetFrame } from "./WidgetFrame";
import { WIDGET_TEMPLATES } from "./gallery";
import { QUERY_INFO, isWidgetQuery, type WidgetSources } from "./catalog";
import { refreshWidgets, widgetsApi, widgetsSlice, type WidgetsApi } from "./api";

const head = { display: "flex", alignItems: "center", padding: "12px 16px", borderBottom: "1px solid var(--line-1)" } as const;
const small = { fontSize: 11, color: "var(--tx-3)", lineHeight: 1.45 } as const;
const AUTHOR: Record<string, string> = { member: "you", hermes: "Hermes", gallery: "gallery" };

export function WidgetsPanel({ sources, api = widgetsApi() }: { sources: WidgetSources; api?: WidgetsApi | null }) {
  const st = widgetsSlice.use();
  const [gallery, setGallery] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [source, setSource] = useState<{ id: string; text: string } | null>(null);

  useEffect(() => {
    if (api && !st.loaded) void refreshWidgets();
  }, [api, st.loaded]);

  if (!api) {
    return (
      <div className="surface" data-testid="widgets-panel">
        <div style={head}>
          <span style={{ fontSize: 13.5, fontWeight: 500 }}>Widgets</span>
        </div>
        <p style={{ ...small, margin: 0, padding: 16 }}>Widgets run in the desktop app.</p>
      </div>
    );
  }

  const run = async (key: string, f: () => Promise<unknown>) => {
    setBusy(key);
    setErr(null);
    try {
      await f();
      await refreshWidgets();
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(null);
    }
  };

  return (
    <div className="surface" data-testid="widgets-panel" style={{ display: "flex", flexDirection: "column" }}>
      <div style={head}>
        <span style={{ fontSize: 13.5, fontWeight: 500 }}>Widgets</span>
        <button className="btn btn-sm" data-testid="widgets-gallery-toggle" style={{ marginLeft: "auto" }} onClick={() => setGallery((g) => !g)}>
          {gallery ? "Close gallery" : "Gallery"}
        </button>
      </div>
      {err ? (
        <p role="alert" style={{ ...small, color: "var(--danger)", margin: 0, padding: "8px 16px" }}>
          {err}
        </p>
      ) : null}
      {st.error ? (
        <p role="alert" style={{ ...small, color: "var(--danger)", margin: 0, padding: "8px 16px" }}>
          {st.error}
        </p>
      ) : null}
      {gallery ? (
        <div data-testid="widgets-gallery" style={{ padding: "8px 16px", borderBottom: "1px solid var(--line-1)", display: "flex", flexDirection: "column", gap: 8 }}>
          {WIDGET_TEMPLATES.map((t) => (
            <div key={t.key} data-testid="widgets-template" style={{ display: "flex", gap: 8, alignItems: "center" }}>
              <span style={{ flex: 1, minWidth: 0 }}>
                <span style={{ display: "block", fontSize: 12.5 }}>{t.name}</span>
                <span style={small}>{t.description}</span>
              </span>
              <button
                className="btn btn-sm"
                disabled={busy !== null}
                onClick={() => void run("add:" + t.key, () => api.save({ name: t.name, description: t.description, html: t.html, queries: [...t.queries], author: "gallery" }))}
              >
                Add
              </button>
            </div>
          ))}
          <span style={small}>Or ask Hermes in chat, for example "make me a widget that shows my node's peers". You approve its source before it is saved.</span>
        </div>
      ) : null}
      {st.list.length === 0 ? (
        <p style={{ ...small, margin: 0, padding: 16 }}>No widgets yet. Add one from the gallery, or ask Hermes to make one.</p>
      ) : (
        st.list.map((w) => (
          <div key={w.id} data-testid="widget-tile" style={{ borderBottom: "1px solid var(--line-1)" }}>
            <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "8px 16px 0" }}>
              <span style={{ fontSize: 12.5, fontWeight: 500, flex: 1, minWidth: 0, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{w.name}</span>
              <span className="mono" style={{ fontSize: 9.5, color: "var(--tx-3)" }}>by {AUTHOR[w.author] ?? w.author}</span>
              <button
                className="btn btn-sm"
                onClick={() =>
                  source?.id === w.id
                    ? setSource(null)
                    : void api.source(w.id).then(
                        (text) => setSource({ id: w.id, text }),
                        (e) => setErr(String(e)),
                      )
                }
              >
                {source?.id === w.id ? "Hide source" : "Source"}
              </button>
              <button className="btn btn-sm" data-testid="widget-remove" disabled={busy !== null} onClick={() => void run("rm:" + w.id, () => api.remove(w.id))}>
                Remove
              </button>
            </div>
            <WidgetFrame widget={w} sources={sources} />
            <div style={{ ...small, padding: "0 16px 8px" }}>
              Reads: {w.queries.length ? w.queries.map((q) => (isWidgetQuery(q) ? QUERY_INFO[q] : q)).join("; ") : "nothing"}. No network, no app commands.
            </div>
            {source?.id === w.id ? (
              <pre data-testid="widget-source" style={{ margin: "0 16px 8px", maxHeight: 160, overflow: "auto", fontSize: 10.5, whiteSpace: "pre-wrap", background: "var(--srf-1)", padding: 8, borderRadius: 6 }}>
                {source.text}
              </pre>
            ) : null}
          </div>
        ))
      )}
    </div>
  );
}
