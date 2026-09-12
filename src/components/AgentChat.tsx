// =====================================================================
// AgentChat — the shared agent chat pane (transcript + model-router picker + on-device
// voice + streaming auto-scroll + honest backend label). Extracted from the Dashboard so
// the SAME chat mounts on both the Dashboard and the Agent surface (#62 — the Agent
// section had no chat). All state is store-level (store.sendChat / chatMsgs / chatInputEl /
// chatScrollEl / routerActive / selectModel), so this component is a thin, surface-agnostic
// view over it. Nothing here signs or fabricates a reply (Rule 1/3) — the send path routes
// through the store's honest provider selection.
// =====================================================================
import { useEffect, useRef, useState } from "react";
import { LoaderMark } from "./LoaderMark";
import { ModelPicker } from "./ModelPicker";
import { modelsSlice, selectModel as sliceSelectModel, refreshRegistryModels, refreshLocalModels } from "../shell/slices/models";
import { choicesFromSources, registryModelsToChoiceInput } from "../agent/modelRouterSources";
import { appendFinal, createDictation, type Dictation } from "../agent/dictation";
import type { Store } from "../shell/store";
import type { AppState } from "../shell/state";

const SUGGESTIONS = ["What is my staking position?", "Break down my earnings", "Journal: node held through the night", "Network status"];

// Chat runs on the built-in local demo agent today — neither the gateway nor a real local
// model is wired for inference yet (Settings → AI providers says so). Honest label (Rule 1).
const CHAT_BACKEND_LABEL = "local demo agent · preview";
const CHAT_DOT_COLOR = "#ffbd10";

export function AgentChat({ store, s }: { store: Store; s: AppState }) {
  const models = modelsSlice.use();
  const chatThinking = s.chatStatus === "thinking" || s.chatStatus === "tool";
  const chatThinkingLabel = s.chatStatus === "tool" ? "running tools" : "reasoning";
  const chatBusy = s.chatStatus !== "ready";
  const showSuggestions = s.chatMsgs.length <= 1 && s.chatStatus === "ready";
  const modelDownloading = s.modelState === "downloading" || s.modelState === "verifying";
  const modelPct = s.modelTotalBytes > 0 ? Math.min(100, Math.round((s.modelDownloadedBytes / s.modelTotalBytes) * 100)) : 0;

  // WP0.4 — the ModelRouter picker over the LIVE sources (local verified models + the on-chain
  // registry + the always-ready gateway). The chat + the Models section share one source of truth.
  const routerChoices = choicesFromSources(models.local, registryModelsToChoiceInput(models.registry));
  const activeModel = store.routerActive(routerChoices);
  const [pickerOpen, setPickerOpen] = useState(false);
  const onPickModel = (id: string) => {
    store.selectModel(id, routerChoices);
    const chosen = routerChoices.find((c) => c.id === id);
    if (chosen?.source === "local") void sliceSelectModel(id); // switch the SERVED local model
    setPickerOpen(false);
  };
  useEffect(() => {
    void refreshLocalModels();
    void refreshRegistryModels();
  }, []);

  const onSend = () => store.sendChat(store.chatInputEl ? store.chatInputEl.value : "");

  // WP4.1 — on-device browser dictation; honest fallback elsewhere.
  const dictation = useRef<Dictation | null>(null);
  const [micOn, setMicOn] = useState(false);
  const [micInterim, setMicInterim] = useState("");
  useEffect(() => () => dictation.current?.stop(), []);
  const toggleMic = () => {
    if (!dictation.current) {
      dictation.current = createDictation({
        onFinal: (frag) => {
          const el = store.chatInputEl;
          if (el) el.value = appendFinal(el.value, frag);
          setMicInterim("");
        },
        onInterim: (t) => setMicInterim(t),
        onError: (m) => { setMicOn(false); setMicInterim(""); store.toast(m); },
        onEnd: () => setMicInterim(""),
      });
    }
    const d = dictation.current;
    if (!d.supported) {
      store.toast("Dictation needs Chrome or Edge here — typing works everywhere");
      return;
    }
    if (micOn) { d.stop(); setMicOn(false); setMicInterim(""); }
    else { d.start(); setMicOn(true); }
  };

  // Post-commit auto-scroll (keyed on chatMsgs) so the stream follows; gated on near-bottom.
  useEffect(() => {
    const el = store.chatScrollEl;
    if (!el) return;
    const distanceFromBottom = el.scrollHeight - el.scrollTop - el.clientHeight;
    if (distanceFromBottom < 160) el.scrollTop = el.scrollHeight;
  }, [s.chatMsgs, store]);

  return (
    <div className="surface" style={{ display: "flex", flexDirection: "column", minHeight: 0, overflow: "hidden", flex: 1 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 10, padding: "12px 16px", borderBottom: "1px solid var(--line-1)" }}>
        <span style={{ fontSize: 14, fontWeight: 500 }}>Agent</span>
        <span style={{ flex: 1 }}></span>
        <span className="mono" style={{ fontSize: 10, letterSpacing: ".08em", color: "var(--tx-3)", display: "inline-flex", alignItems: "center", gap: 6 }}>
          <span style={{ width: 6, height: 6, borderRadius: 999, background: CHAT_DOT_COLOR }}></span>
          {CHAT_BACKEND_LABEL}
        </span>
        {modelDownloading && (
          <span className="mono" style={{ fontSize: 10, letterSpacing: ".08em", color: "var(--accent-text)", display: "inline-flex", alignItems: "center", gap: 5 }} data-testid="dash-model-dl">
            <span style={{ width: 6, height: 6, borderRadius: 999, background: "var(--accent)" }}></span>
            {s.modelState === "verifying" ? "local model · verifying" : `local model · ${modelPct}%`}
          </span>
        )}
        <button
          className="mono"
          data-testid="model-chip"
          aria-expanded={pickerOpen}
          onClick={() => setPickerOpen((v) => !v)}
          style={{ fontSize: 10, letterSpacing: ".06em", textTransform: "uppercase", color: "var(--tx-2)", background: "var(--srf-2)", border: "1px solid var(--line-1)", borderRadius: 999, padding: "3px 9px", cursor: "pointer", display: "inline-flex", alignItems: "center", gap: 5 }}
        >
          <span style={{ width: 6, height: 6, borderRadius: 999, background: "var(--accent)" }} aria-hidden></span>
          {activeModel.label}
          <span aria-hidden style={{ color: "var(--tx-3)" }}>{pickerOpen ? "▴" : "▾"}</span>
        </button>
      </div>
      {pickerOpen && (
        <div style={{ padding: 12, borderBottom: "1px solid var(--line-1)", background: "var(--srf-1)" }}>
          <ModelPicker choices={routerChoices} activeId={s.activeModelId} onSelect={onPickModel} />
        </div>
      )}
      <div ref={(el) => { store.chatScrollEl = el; }} style={{ flex: 1, minHeight: 0, overflow: "auto", padding: 16, display: "flex", flexDirection: "column", gap: 16 }}>
        {s.chatMsgs.map((m) => (
          <div key={m.id} style={{ display: "flex", flexDirection: "column", gap: 5 }}>
            <span className="mono" style={{ fontSize: 9.5, letterSpacing: ".13em", textTransform: "uppercase", color: m.who === "You" ? "var(--tx-3)" : "var(--accent-text)" }}>
              {m.who}
            </span>
            <span style={{ fontSize: 13.5, lineHeight: 1.6, color: "var(--tx-1)", whiteSpace: "pre-wrap" }}>
              {m.text}
              {m.streaming && <span style={{ display: "inline-block", width: 7, height: 14, background: "var(--accent)", marginLeft: 2, verticalAlign: -2, animation: "ccCaret 1s step-end infinite" }}></span>}
            </span>
            {m.chips && m.chips.length > 0 && (
              <span style={{ display: "flex", gap: 6, flexWrap: "wrap", marginTop: 2 }}>
                {m.chips.map((c, i) => {
                  const bd = c.status === "declined" ? "var(--danger)" : c.status === "approved" ? "var(--ok)" : "var(--line-2)";
                  const fg = c.status === "declined" ? "var(--danger)" : c.status === "approved" ? "var(--ok)" : "var(--tx-2)";
                  return (
                    <span key={i} className="mono" style={{ fontSize: 10, letterSpacing: ".05em", padding: "2px 8px", borderRadius: 999, border: "1px solid " + bd, color: fg, background: "transparent" }}>
                      {c.label}
                    </span>
                  );
                })}
              </span>
            )}
          </div>
        ))}
        {chatThinking && (
          <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
            <span style={{ width: 34, height: 34, display: "inline-block", flexShrink: 0 }}>
              <LoaderMark size={34} />
            </span>
            <span className="mono" style={{ fontSize: 10.5, letterSpacing: ".1em", textTransform: "uppercase", color: "var(--tx-3)" }}>
              {chatThinkingLabel}
            </span>
          </div>
        )}
      </div>
      {showSuggestions && (
        <div style={{ display: "flex", gap: 8, flexWrap: "wrap", padding: "0 16px 10px" }}>
          {SUGGESTIONS.map((label) => (
            <button
              key={label}
              onClick={() => store.sendChat(label)}
              style={{ fontFamily: "var(--font-sans)", fontSize: 12, color: "var(--tx-2)", background: "var(--srf-1)", border: "1px solid var(--line-1)", borderRadius: 999, padding: "5px 12px", cursor: "pointer" }}
            >
              {label}
            </button>
          ))}
        </div>
      )}
      <div style={{ display: "flex", gap: 10, padding: "12px 16px", borderTop: "1px solid var(--line-1)", alignItems: "center" }}>
        <span style={{ flex: 1, position: "relative", display: "flex" }}>
          <input
            ref={(el) => { store.chatInputEl = el; }}
            className="input"
            placeholder={micOn ? (micInterim || "Listening…") : "Ask about your node, wallet, or memory…"}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !e.shiftKey) {
                e.preventDefault();
                store.sendChat((e.target as HTMLInputElement).value);
              }
            }}
            style={{ flex: 1 }}
          />
        </span>
        <button
          className={"btn " + (micOn ? "btn-secondary" : "btn-ghost")}
          onClick={toggleMic}
          title={micOn ? "Stop dictation" : "Dictate (on-device)"}
          aria-pressed={micOn}
          style={micOn ? { color: "var(--ok)", borderColor: "var(--ok)" } : undefined}
        >
          {micOn ? "● Mic" : "Mic"}
        </button>
        <button className="btn btn-primary" onClick={onSend} disabled={chatBusy}>
          Send
        </button>
      </div>
    </div>
  );
}
