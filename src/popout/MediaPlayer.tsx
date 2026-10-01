// =====================================================================
// citrate-core — Media player pop-out (HUP-S10.1, US-10.1 AC2)
//
// The gallery of things Hermes made for the member, one image shown large, its cost line, and
// "save a copy" into a folder the member granted for writing. The pop-out has no app commands:
// everything comes from the main window over the media bridge. Nothing is invented: a file that
// is gone says so, and a failed save shows the reason.
// =====================================================================
import { useEffect, useRef, useState } from "react";
import type { BridgeTransport } from "./bridge";
import { createMediaPopoutEnd, type MediaCard, type MediaPopoutEnd, type MediaSaveTarget } from "./mediaBridge";

const shell = {
  minHeight: "100vh",
  boxSizing: "border-box" as const,
  padding: "14px 16px",
  background: "var(--srf-0)",
  color: "var(--tx-1)",
  fontFamily: "var(--font-sans)",
  fontSize: 13,
  display: "flex",
  flexDirection: "column" as const,
  gap: 10,
};

const fmtTime = (secs: number) => new Date(secs * 1000).toLocaleString();

export function MediaPlayer({ transport }: { transport: () => Promise<BridgeTransport> }) {
  const [items, setItems] = useState<MediaCard[] | null>(null);
  const [targets, setTargets] = useState<MediaSaveTarget[]>([]);
  const [note, setNote] = useState<string | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [image, setImage] = useState<{ id: string; dataUrl: string } | null>(null);
  const [result, setResult] = useState<{ ok: boolean; message: string } | null>(null);
  const [target, setTarget] = useState("");
  const [failed, setFailed] = useState<string | null>(null);
  const end = useRef<MediaPopoutEnd | null>(null);

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const t = await transport();
        const e = await createMediaPopoutEnd(t, (m) => {
          if (m.type === "media.gallery") {
            setItems(m.items);
            setTargets(m.targets);
            setNote(m.note);
          } else if (m.type === "media.image") {
            setImage({ id: m.id, dataUrl: m.dataUrl });
          } else {
            setResult({ ok: m.ok, message: m.message });
          }
        });
        if (cancelled) {
          e.close();
          return;
        }
        end.current = e;
        await e.ready();
      } catch (err) {
        if (!cancelled) setFailed(err instanceof Error ? err.message : String(err));
      }
    })();
    return () => {
      cancelled = true;
      end.current?.close();
      end.current = null;
    };
  }, [transport]);

  // Show the newest item first time the gallery arrives.
  useEffect(() => {
    if (selected === null && items && items.length > 0) {
      const first = items.find((i) => i.present && i.kind === "image");
      if (first) {
        setSelected(first.id);
        void end.current?.show(first.id);
      }
    }
  }, [items, selected]);

  useEffect(() => {
    if (!target && targets.length > 0) setTarget(targets[0].grantId);
  }, [targets, target]);

  if (failed) {
    return (
      <div data-register="instrument" role="alert" style={shell}>
        The Media player could not connect to the main window: {failed}
      </div>
    );
  }
  if (!items) {
    return (
      <div data-register="instrument" style={shell}>
        Waiting for the main window…
      </div>
    );
  }

  const pick = (id: string) => {
    setSelected(id);
    setResult(null);
    void end.current?.show(id);
  };
  const sel = items.find((i) => i.id === selected) ?? null;

  return (
    <div data-testid="media-player" data-register="instrument" style={shell}>
      <span style={{ fontSize: 14, fontWeight: 500 }}>Media player</span>
      {note && (
        <div role="status" data-testid="media-note" style={{ fontSize: 12, color: "var(--warn)" }}>
          {note}
        </div>
      )}
      {items.length === 0 ? (
        <div data-testid="media-empty" style={{ color: "var(--tx-2)" }}>
          Nothing has been made yet. Generate an image from Files in the main window; it opens here.
        </div>
      ) : (
        <div style={{ display: "grid", gridTemplateColumns: "200px minmax(0,1fr)", gap: 12, flex: 1, minHeight: 0 }}>
          <ul aria-label="Gallery" style={{ listStyle: "none", margin: 0, padding: 0, overflowY: "auto", display: "flex", flexDirection: "column", gap: 4 }}>
            {items.map((i) => (
              <li key={i.id}>
                <button
                  data-testid="media-item"
                  className="btn btn-ghost btn-sm"
                  aria-pressed={i.id === selected}
                  disabled={!i.present}
                  onClick={() => pick(i.id)}
                  title={i.present ? i.prompt : "This file is no longer in its folder"}
                  style={{ width: "100%", textAlign: "left", justifyContent: "flex-start", overflow: "hidden", whiteSpace: "nowrap", textOverflow: "ellipsis" }}
                >
                  {i.present ? i.prompt || i.name : "(missing) " + i.name}
                </button>
              </li>
            ))}
          </ul>
          <div style={{ display: "flex", flexDirection: "column", gap: 8, minWidth: 0 }}>
            {sel && image && image.id === sel.id ? (
              <img data-testid="media-image" src={image.dataUrl} alt={sel.prompt} style={{ maxWidth: "100%", maxHeight: "60vh", objectFit: "contain", borderRadius: 6, background: "var(--srf-1)" }} />
            ) : sel ? (
              <div style={{ color: "var(--tx-3)" }}>Loading…</div>
            ) : null}
            {sel && (
              <div style={{ display: "flex", flexDirection: "column", gap: 3, fontSize: 12, color: "var(--tx-2)" }}>
                <span data-testid="media-meta">
                  {sel.name} · {fmtTime(sel.createdAt)} · made on {sel.destination}
                </span>
                <span data-testid="media-cost">Cost: {sel.cost}{sel.usage ? ". " + sel.usage : ""}</span>
              </div>
            )}
            {sel && (
              <div style={{ display: "flex", gap: 8, alignItems: "center", flexWrap: "wrap" }}>
                {targets.length === 0 ? (
                  <span data-testid="media-no-targets" style={{ fontSize: 12, color: "var(--tx-3)" }}>
                    To save a copy, grant Hermes a folder for writing in Settings, App, Hermes folder access.
                  </span>
                ) : (
                  <>
                    <label style={{ fontSize: 12 }}>
                      Save a copy to{" "}
                      <select data-testid="media-target" value={target} onChange={(e) => setTarget(e.target.value)}>
                        {targets.map((t) => (
                          <option key={t.grantId} value={t.grantId}>
                            {t.root}
                          </option>
                        ))}
                      </select>
                    </label>
                    <button data-testid="media-save" className="btn btn-sm" disabled={!target || !sel.present} onClick={() => void end.current?.save(sel.id, target)}>
                      Save
                    </button>
                  </>
                )}
              </div>
            )}
            {result && (
              <div role="status" data-testid="media-result" style={{ fontSize: 12, color: result.ok ? "var(--ok)" : "var(--danger)" }}>
                {result.message}
              </div>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
