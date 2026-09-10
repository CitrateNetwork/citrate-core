// =====================================================================
// citrate-core — Models (CX-S1.6, lane s1)
//
// The model switcher: your locally-present models plus downloadable ones from Hugging Face and
// GitHub. Real data via bridge.modelsCatalog (S1.4 resolver + S1.5 download/select) folded through
// the models slice. Honest states throughout (Rule 1): empty when there is nothing, the real error
// text when a call fails, and never a fabricated catalog. Grandma-proof: plain words, one action
// per row, one thing downloading at a time.
// =====================================================================
import { useEffect, useState } from "react";
import { SurfaceProps } from "./shared";
import {
  modelsSlice,
  refreshLocalModels,
  searchModels,
  downloadModel,
  selectModel,
  type ModelSourceId,
} from "../shell/slices/models";
import type { ModelDescriptor } from "../bridge/domains";

function humanBytes(n: number): string {
  if (!n || n < 0) return "—";
  const gb = n / 1e9;
  if (gb >= 1) return `${gb.toFixed(gb >= 10 ? 0 : 1)} GB`;
  return `${Math.max(1, Math.round(n / 1e6))} MB`;
}

const SOURCE_LABEL: Record<ModelSourceId, string> = {
  hf: "Hugging Face",
  github: "GitHub",
};

export function Models({ store }: SurfaceProps) {
  const st = modelsSlice.use();
  const [source, setSource] = useState<ModelSourceId>("hf");
  const [query, setQuery] = useState("");

  // Load the locally-present models once on mount (honest-empty on a sim/unwired bridge).
  useEffect(() => {
    void refreshLocalModels();
  }, []);

  const installedFiles = new Set(st.local.map((m) => m.file));

  const runSearch = () => {
    if (query.trim() === "") return store.toast("Type what you are looking for first");
    void searchModels(source, query);
  };

  return (
    <div style={{ padding: "20px 26px 24px", display: "flex", flexDirection: "column", gap: 18, maxWidth: 820 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 18 }}>
        <span style={{ fontFamily: "var(--font-display)", fontWeight: 420, fontSize: 24 }}>Models</span>
        <span className="mono" style={{ marginLeft: "auto", fontSize: 10, letterSpacing: ".06em", color: "var(--tx-3)" }}>
          the local model powers chat and your agent
        </span>
      </div>

      {st.error && (
        <div
          className="surface"
          role="alert"
          style={{ padding: "12px 16px", fontSize: 12.5, color: "var(--danger)", lineHeight: 1.5 }}
        >
          {st.error}
        </div>
      )}

      {/* ---- active model ---- */}
      {(() => {
        const active = st.local.find((m) => m.id === st.activeId) ?? null;
        return (
          <div className="surface" style={{ padding: "16px 18px", display: "flex", alignItems: "center", gap: 14 }}>
            <span style={{ width: 10, height: 10, borderRadius: 999, background: active ? "var(--ok)" : "var(--tx-3)", flexShrink: 0 }}></span>
            <span style={{ flex: 1, minWidth: 0 }}>
              <span className="mono" style={{ display: "block", fontSize: 13, fontWeight: 500 }}>{active ? active.file : "No active model selected"}</span>
              <span className="mono" style={{ display: "block", fontSize: 10.5, color: "var(--tx-3)", marginTop: 2 }}>
                {active ? `${active.repo || active.source} · ${humanBytes(active.sizeBytes)}` : "pick a model below to power chat and your agent"}
              </span>
            </span>
            <span className="eyebrow">{active ? "in use" : "idle"}</span>
          </div>
        );
      })()}

      {/* ---- your models ---- */}
      <section style={{ display: "flex", flexDirection: "column", gap: 8 }}>
        <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
          <span style={{ fontSize: 13, fontWeight: 520 }}>On this machine</span>
          {st.localPending && (
            <span
              className="mono"
              title="modelsCatalog.local() isn't wired yet — this list can't be read"
              style={{ fontSize: 9, letterSpacing: ".1em", textTransform: "uppercase", padding: "2px 8px", borderRadius: 999, border: "1px solid var(--warn)", color: "var(--warn)", background: "var(--warn-bg)" }}
            >
              pending wire · local()
            </span>
          )}
        </div>
        <div className="surface" style={{ display: "flex", flexDirection: "column" }}>
          {st.local.length === 0 ? (
            st.localPending ? (
              <div style={{ padding: "18px", fontSize: 12.5, color: "var(--tx-3)", lineHeight: 1.6 }}>
                Reading your installed models isn't wired in this build yet. When it lands, the models already on this machine show here — nothing is invented in the meantime. You can still search and download below.
              </div>
            ) : (
              <div style={{ padding: "18px", fontSize: 12.5, color: "var(--tx-3)", lineHeight: 1.6 }}>
                No models installed yet. Search below and download one to run it on this machine.
              </div>
            )
          ) : (
            st.local.map((m) => (
              <ModelRow
                key={m.id}
                model={m}
                active={st.activeId === m.id}
                busy={st.selectingId === m.id}
                actionLabel={st.activeId === m.id ? "In use" : st.selectingId === m.id ? "Switching…" : "Use"}
                actionDisabled={st.activeId === m.id || st.selectingId != null}
                onAction={() => void selectModel(m.id)}
              />
            ))
          )}
        </div>
      </section>

      {/* ---- add a model ---- */}
      <section style={{ display: "flex", flexDirection: "column", gap: 8 }}>
        <div style={{ fontSize: 13, fontWeight: 520 }}>Add a model</div>
        <div style={{ display: "flex", gap: 8, alignItems: "center", flexWrap: "wrap" }}>
          {(["hf", "github"] as const).map((sid) => (
            <button
              key={sid}
              className={source === sid ? "btn btn-sm" : "btn btn-ghost btn-sm"}
              onClick={() => setSource(sid)}
            >
              {SOURCE_LABEL[sid]}
            </button>
          ))}
          <input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") runSearch();
            }}
            placeholder={source === "github" ? "owner/repo (e.g. TheBloke/Llama-2-7B-GGUF)" : "search models (e.g. gemma gguf)"}
            aria-label="model search"
            style={{ flex: 1, minWidth: 220, padding: "7px 10px", fontSize: 12.5 }}
          />
          <button className="btn btn-sm" onClick={runSearch} disabled={st.searching}>
            {st.searching ? "Searching…" : "Search"}
          </button>
        </div>

        <div className="surface" style={{ display: "flex", flexDirection: "column" }}>
          {st.results.length === 0 ? (
            <div style={{ padding: "18px", fontSize: 12.5, color: "var(--tx-3)", lineHeight: 1.6 }}>
              {st.searching
                ? "Searching…"
                : "Results appear here. Only models we can verify (a published checksum) are shown."}
            </div>
          ) : (
            st.results.map((m) => {
              const installed = installedFiles.has(m.file);
              const downloading = st.downloadingId === m.id;
              return (
                <ModelRow
                  key={m.id}
                  model={m}
                  active={false}
                  busy={downloading}
                  actionLabel={
                    installed
                      ? "Installed"
                      : downloading
                        ? st.downloadPct != null
                          ? `Downloading… ${st.downloadPct}%`
                          : "Downloading…"
                        : "Download"
                  }
                  actionDisabled={installed || downloading || st.downloadingId != null}
                  onAction={() => void downloadModel(m.id)}
                />
              );
            })
          )}
        </div>
        <div style={{ fontSize: 10.5, color: "var(--tx-3)", lineHeight: 1.5 }}>
          Downloads verify against the source's published checksum before they can be used — a file
          that does not match is discarded, never run.
        </div>
      </section>
    </div>
  );
}

function ModelRow({
  model,
  active,
  busy,
  actionLabel,
  actionDisabled,
  onAction,
}: {
  model: ModelDescriptor;
  active: boolean;
  busy: boolean;
  actionLabel: string;
  actionDisabled: boolean;
  onAction: () => void;
}) {
  return (
    <div
      style={{
        display: "flex",
        alignItems: "center",
        gap: 12,
        padding: "12px 16px",
        borderTop: "1px solid var(--ln, rgba(0,0,0,0.06))",
      }}
    >
      <div style={{ display: "flex", flexDirection: "column", gap: 2, minWidth: 0, flex: 1 }}>
        <span style={{ fontSize: 12.5, fontWeight: 500, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
          {model.file}
        </span>
        <span className="mono" style={{ fontSize: 10.5, color: "var(--tx-3)" }}>
          {model.repo || model.source} · {humanBytes(model.sizeBytes)}
          {active ? " · active" : ""}
        </span>
      </div>
      <button
        className={active ? "btn btn-ghost btn-sm" : "btn btn-sm"}
        onClick={onAction}
        disabled={actionDisabled}
        aria-busy={busy}
        style={actionDisabled ? { opacity: 0.55 } : undefined}
      >
        {actionLabel}
      </button>
    </div>
  );
}
