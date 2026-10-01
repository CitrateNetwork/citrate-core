// =====================================================================
// HUP-S10.1 — the Media panel (Files > Media), US-10.1.
//
// Make an image on this device or through one of the member's AI providers, see what each route
// costs before anything runs, and save the result into a folder the member granted for writing.
// Results open in the Media player pop-out. Video shows its honest state: no backend in this
// build. Every unavailable route says why (Rule 1).
// =====================================================================
import { useCallback, useEffect, useState } from "react";
import { MEDIA_DESKTOP_ONLY, baseName, errorMessage, usageLine, type GalleryItem, type MediaIo, type MediaOptions, type MediaSettings } from "./media";

export interface MediaPanelProps {
  io: () => Promise<MediaIo>;
  /** Open the Media player pop-out (injected; the app passes openPopout("media")). */
  openPlayer: () => void | Promise<void>;
  /** Tell the pop-out something new exists (injected; the app refreshes the media host). */
  onSaved?: () => void;
}

const row = { display: "flex", gap: 8, alignItems: "center", flexWrap: "wrap" as const };
const note = { fontSize: 12, color: "var(--tx-2)", lineHeight: 1.5 };
const PROVIDERS: [string, string][] = [
  ["", "none"],
  ["openai", "OpenAI"],
  ["gateway", "Citrate gateway"],
  ["custom", "OpenAI-compatible endpoint"],
];

export function MediaPanel({ io, openPlayer, onSaved }: MediaPanelProps) {
  const [x, setX] = useState<MediaIo | null>(null);
  const [opts, setOpts] = useState<MediaOptions | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [prompt, setPrompt] = useState("");
  const [route, setRoute] = useState("");
  const [size, setSize] = useState("1024x1024");
  const [target, setTarget] = useState("");
  const [busy, setBusy] = useState(false);
  const [last, setLast] = useState<{ item: GalleryItem; dataUrl: string | null } | null>(null);
  const [editing, setEditing] = useState<MediaSettings | null>(null);

  const load = useCallback(async (m: MediaIo) => {
    try {
      const o = await m.invoke<MediaOptions>("media_options", {});
      setOpts(o);
      setErr(null);
      setRoute((r) => (r && o.image.some((x) => x.id === r && x.available) ? r : o.image.find((x) => x.available)?.id ?? ""));
      setTarget((t) => (t && o.targets.some((x) => x.grantId === t) ? t : o.targets[0]?.grantId ?? ""));
    } catch (e) {
      setErr(errorMessage(e));
    }
  }, []);

  useEffect(() => {
    let live = true;
    void io().then((m) => {
      if (!live) return;
      setX(m);
      if (m.mode === "tauri") void load(m);
    });
    return () => {
      live = false;
    };
  }, [io, load]);

  if (x && x.mode !== "tauri") {
    return (
      <div data-testid="media-desktop-only" style={note}>
        {MEDIA_DESKTOP_ONLY}
      </div>
    );
  }
  if (!opts) {
    return <div style={note}>{err ? "Media options could not be read: " + err : "Loading…"}</div>;
  }

  const chosen = opts.image.find((r) => r.id === route) ?? null;
  const canGenerate = !!x && !busy && !!chosen?.available && !!target && prompt.trim().length > 0;

  const generate = async () => {
    if (!x || !chosen) return;
    setBusy(true);
    setErr(null);
    try {
      const item = await x.invoke<GalleryItem>("media_generate_image", { route, prompt, size, grantId: target });
      let dataUrl: string | null = null;
      try {
        dataUrl = await x.invoke<string>("media_read", { id: item.id });
      } catch {
        dataUrl = null;
      }
      setLast({ item, dataUrl });
      onSaved?.();
    } catch (e) {
      setErr(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  const saveSettings = async () => {
    if (!x || !editing) return;
    setErr(null);
    try {
      await x.invoke<MediaSettings>("media_set_settings", { settings: editing });
      setEditing(null);
      await load(x);
    } catch (e) {
      setErr(errorMessage(e));
    }
  };

  return (
    <div data-testid="media-panel" style={{ display: "flex", flexDirection: "column", gap: 10 }}>
      <div style={row}>
        <span style={note} data-testid="media-tier">
          This device: {opts.tier ?? "tier unknown"} · local images {opts.caps.localImage ? "allowed" : "not available"} · local video{" "}
          {opts.caps.localVideo ? "allowed by the tier" : "not available"}
        </span>
        <button className="btn btn-ghost btn-sm" style={{ marginLeft: "auto" }} onClick={() => void openPlayer()} data-testid="media-open-player">
          Open Media player
        </button>
      </div>

      <ul style={{ listStyle: "none", margin: 0, padding: 0, display: "flex", flexDirection: "column", gap: 6 }}>
        {[...opts.image, ...opts.video].map((r) => (
          <li key={r.kind + r.id} data-testid={`media-route-${r.kind}-${r.id}`} style={{ fontSize: 12.5, display: "flex", flexDirection: "column", gap: 2 }}>
            <span>
              <strong>{r.kind === "image" ? "Image" : "Video"}</strong> · {r.id === "local" ? "on this device" : "through a provider"} ·{" "}
              <span style={{ color: r.available ? "var(--ok)" : "var(--tx-3)" }}>{r.available ? "available" : "not available"}</span>
              {r.model ? ` · model ${r.model}` : ""}
            </span>
            {r.reason && <span style={note}>{r.reason}</span>}
            <span style={note}>Cost: {r.cost}</span>
          </li>
        ))}
      </ul>

      {editing ? (
        <div data-testid="media-settings" style={{ display: "flex", flexDirection: "column", gap: 6 }}>
          <label style={note}>
            Local image server (an http address on this machine that speaks the OpenAI images API)
            <input className="input" value={editing.localUrl ?? ""} placeholder="http://127.0.0.1:7860/v1" onChange={(e) => setEditing({ ...editing, localUrl: e.target.value })} />
          </label>
          <label style={note}>
            Local model
            <input className="input" value={editing.localModel ?? ""} onChange={(e) => setEditing({ ...editing, localModel: e.target.value })} />
          </label>
          <label style={note}>
            Provider for images (uses the key you added in Settings, AI providers)
            <select value={editing.remoteProvider ?? ""} onChange={(e) => setEditing({ ...editing, remoteProvider: e.target.value || null })}>
              {PROVIDERS.map(([id, name]) => (
                <option key={id} value={id}>
                  {name}
                </option>
              ))}
            </select>
          </label>
          <label style={note}>
            Image model at that provider
            <input className="input" value={editing.remoteModel ?? ""} placeholder="gpt-image-1" onChange={(e) => setEditing({ ...editing, remoteModel: e.target.value })} />
          </label>
          <div style={row}>
            <button className="btn btn-sm" onClick={() => void saveSettings()} data-testid="media-settings-save">
              Save
            </button>
            <button className="btn btn-ghost btn-sm" onClick={() => setEditing(null)}>
              Cancel
            </button>
          </div>
        </div>
      ) : (
        <div>
          <button className="btn btn-ghost btn-sm" onClick={() => setEditing(opts.settings)} data-testid="media-settings-edit">
            Media settings
          </button>
        </div>
      )}

      <label style={note}>
        Describe the image
        <textarea data-testid="media-prompt" className="input" rows={3} value={prompt} onChange={(e) => setPrompt(e.target.value)} />
      </label>
      <div style={row}>
        <label style={note}>
          Route{" "}
          <select data-testid="media-route" value={route} onChange={(e) => setRoute(e.target.value)}>
            <option value="">choose</option>
            {opts.image.map((r) => (
              <option key={r.id} value={r.id} disabled={!r.available}>
                {r.id === "local" ? "This device" : "Provider (" + r.destination + ")"}
              </option>
            ))}
          </select>
        </label>
        <label style={note}>
          Size{" "}
          <select value={size} onChange={(e) => setSize(e.target.value)}>
            {opts.sizes.map((z) => (
              <option key={z}>{z}</option>
            ))}
          </select>
        </label>
        {opts.targets.length > 0 ? (
          <label style={note}>
            Save to{" "}
            <select data-testid="media-target" value={target} onChange={(e) => setTarget(e.target.value)}>
              {opts.targets.map((t) => (
                <option key={t.grantId} value={t.grantId}>
                  {t.root}
                </option>
              ))}
            </select>
          </label>
        ) : (
          <span data-testid="media-no-target" style={note}>
            {opts.targetsError ?? "Generated files go only into a folder you granted Hermes for writing. Grant one in Settings, App, Hermes folder access."}
          </span>
        )}
      </div>
      {chosen && <span style={note} data-testid="media-cost-now">Before you generate: {chosen.cost}.</span>}
      <div>
        <button className="btn btn-primary btn-sm" disabled={!canGenerate} onClick={() => void generate()} data-testid="media-generate">
          {busy ? "Generating…" : "Generate image"}
        </button>
      </div>
      {err && (
        <div role="alert" data-testid="media-error" style={{ fontSize: 12, color: "var(--danger)" }}>
          {err}
        </div>
      )}
      {last && (
        <div data-testid="media-last" style={{ display: "flex", flexDirection: "column", gap: 4 }}>
          {last.dataUrl && <img src={last.dataUrl} alt={last.item.prompt} style={{ maxWidth: 320, borderRadius: 6 }} />}
          <span style={note}>
            Saved {baseName(last.item.path)} · {last.item.cost}
            {usageLine(last.item.usage) ? ". " + usageLine(last.item.usage) : ""}
          </span>
        </div>
      )}
    </div>
  );
}
