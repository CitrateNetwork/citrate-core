// =====================================================================
// AgentChat — the shared agent chat pane (transcript + model-router picker + on-device
// voice + streaming auto-scroll + honest backend label). Extracted from the Dashboard so
// the SAME chat mounts on both the Dashboard and the Agent surface (#62 — the Agent
// section had no chat). All state is store-level (store.sendChat / chatMsgs / chatInputEl /
// chatScrollEl / routerActive / selectModel), so this component is a thin, surface-agnostic
// view over it. Nothing here signs or fabricates a reply (Rule 1/3) — the send path routes
// through the store's honest provider selection.
// =====================================================================
import { useCallback, useEffect, useRef, useState } from "react";
import { LoaderMark } from "./LoaderMark";
import { Markdown } from "./Markdown";
import { ModelPicker } from "./ModelPicker";
import { InterviewCard, looksLikeBuildAsk } from "./InterviewCard";
import { FileChangeCard } from "./FileChangeCard";
import { agentUndo, undoChange } from "../shell/slices/agentUndo";
import { bridge } from "../bridge";
import { openDiff, openPopout } from "../popout/appHost";
import { BrowserControls } from "./BrowserControls";
import { tauriBrowserApi } from "../popout/browserApi";
import { BRIDGE_MODE } from "../bridge/mode";
import { modelsSlice, selectModel as sliceSelectModel, refreshRegistryModels, refreshLocalModels, refreshCloudModels } from "../shell/slices/models";
import { routerChoicesFor } from "../agent/modelRouterSources";
import { appendFinal, createDictation, type Dictation } from "../agent/dictation";
import type { Store } from "../shell/store";
import type { AppState } from "../shell/state";
import { LocalModelAsk } from "./LocalModelAsk";
import { composerMax, fitComposer, shouldFollow, useFitToViewport } from "./chatLayout";

const SUGGESTIONS = ["What is my staking position?", "Show my validator rewards", "Journal: node held through the night", "Network status"];

// The header dot colour reflects the LIVE provider kind (store.reflectProvider): green when a real
// backend answers (local llama-server or the gateway), amber for the built-in demo. The label text
// comes straight from the active provider — honest about what actually replies (Rule 1).
const DOT_FOR_KIND: Record<string, string> = { local: "#37d67a", real: "#37d67a", demo: "#ffbd10" };

/** 0.5.4: the composer's compact icon glyphs (16 px, currentColor). */
function MicGlyph() {
  return (
    <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" aria-hidden focusable="false">
      <rect x="9" y="3" width="6" height="11" rx="3"></rect>
      <path d="M5 11 A7 7 0 0 0 19 11 M12 18 V21 M9 21 H15"></path>
    </svg>
  );
}
function PlanGlyph() {
  return (
    <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" aria-hidden focusable="false">
      <rect x="5" y="4" width="14" height="17" rx="2"></rect>
      <path d="M9 4 V3 H15 V4 M8.5 10 L10 11.5 L12.5 9 M14 10.5 H16 M8.5 15.5 L10 17 L12.5 14.5 M14 16 H16"></path>
    </svg>
  );
}

export function AgentChat({ store, s }: { store: Store; s: AppState }) {
  const models = modelsSlice.use();
  const undo = agentUndo.use();
  // HUP-S2.3: sign-in requests from the managed browser go to core by id (stable handler).
  const signInHandler = useCallback((id: string) => void store.handleWebSignIn(id), [store]);
  // SCL-S7.5a: a message held for the local model shows its question instead of a spinner.
  const chatThinking = (s.chatStatus === "thinking" || s.chatStatus === "tool") && !s.chatRouteHold;
  const chatThinkingLabel = s.chatStatus === "tool" ? "running tools" : "reasoning";
  const chatBusy = s.chatStatus !== "ready";
  const showSuggestions = s.chatMsgs.length <= 1 && s.chatStatus === "ready";
  const modelDownloading = s.modelState === "downloading" || s.modelState === "verifying";
  const modelPct = s.modelTotalBytes > 0 ? Math.min(100, Math.round((s.modelDownloadedBytes / s.modelTotalBytes) * 100)) : 0;

  // WP0.4 — the ModelRouter picker over the LIVE sources (local verified models + every model of
  // the providers connected in Settings › Models (D-11) + the on-chain registry + the always-ready
  // gateway). The chat + the Models section share one source of truth.
  const routerChoices = routerChoicesFor(models, s.chatRoute);
  const activeModel = store.routerActive(routerChoices);
  const [pickerOpen, setPickerOpen] = useState(false);
  const [pickError, setPickError] = useState<string | null>(null);
  const onPickModel = (id: string) => {
    const chosen = routerChoices.find((c) => c.id === id);
    setPickError(null);
    // D-11: a cloud pick is stored in core and the next turn runs on it through the core proxy.
    void store.selectModel(id, routerChoices).then((err) => setPickError(err));
    if (chosen?.source === "local") void sliceSelectModel(id); // switch the SERVED local model
    setPickerOpen(false);
  };
  useEffect(() => {
    void refreshLocalModels();
    void refreshRegistryModels();
    void refreshCloudModels();
  }, []);
  // A provider connected (or removed) in Settings › Models shows up when the picker opens.
  useEffect(() => {
    if (pickerOpen) void refreshCloudModels();
  }, [pickerOpen]);

  // HUP-S1.4 — "Plan it first": the interview card (track → questions → editable brief). `planGoal`
  // is the goal the card opened with (null = closed). A build-style ask highlights the button; it
  // never hijacks Send.
  const [planGoal, setPlanGoal] = useState<string | null>(null);
  const [buildAsk, setBuildAsk] = useState(false);
  const openPlan = () => setPlanGoal(store.chatInputEl ? store.chatInputEl.value.trim() : "");

  // rc.5 FIX 4: the pane fits the window (composer always visible); only the list scrolls.
  const paneRef = useRef<HTMLDivElement | null>(null);
  const paneHeight = useFitToViewport(paneRef);
  // 0.5.4: the composer grows with what is typed or pasted, up to its cap, then scrolls inside.
  const inputMax = composerMax(paneHeight);
  const fitInput = useCallback(() => fitComposer(store.chatInputEl, inputMax), [store, inputMax]);
  // Follow new messages unless the member scrolled up; sending always jumps back to the latest.
  const follow = useRef(true);
  const send = (text: string) => {
    follow.current = true;
    return store.sendChat(text);
  };

  const onSend = () => {
    setBuildAsk(false);
    void send(store.chatInputEl ? store.chatInputEl.value : "");
  };

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
          fitComposer(el, inputMax);
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

  // Post-commit auto-scroll (keyed on chatMsgs) so the stream follows, unless the member has
  // scrolled up to read (tracked on scroll, so a long streamed chunk does not drop the follow).
  useEffect(() => {
    const el = store.chatScrollEl;
    if (!el || !follow.current) return;
    el.scrollTop = el.scrollHeight;
  }, [s.chatMsgs, store]);
  // A send (or an accepted brief) clears the composer through the store; shrink it back to fit.
  useEffect(() => {
    fitInput();
  }, [s.chatMsgs.length, fitInput]);

  return (
    <div
      ref={paneRef}
      className="surface"
      data-testid="agent-chat"
      style={{ display: "flex", flexDirection: "column", minHeight: 0, overflow: "hidden", flex: "none", height: paneHeight }}
    >
      <div style={{ display: "flex", alignItems: "center", gap: 10, padding: "12px 16px", borderBottom: "1px solid var(--line-1)", flexShrink: 0 }}>
        <span style={{ fontSize: 14, fontWeight: 500 }}>Agent</span>
        <span style={{ flex: 1 }}></span>
        <span className="mono" style={{ fontSize: 10, letterSpacing: ".08em", color: "var(--tx-3)", display: "inline-flex", alignItems: "center", gap: 6 }}>
          <span style={{ width: 6, height: 6, borderRadius: 999, background: DOT_FOR_KIND[s.chatProviderKind] ?? "#ffbd10" }}></span>
          {s.chatProviderLabel}
        </span>
        {modelDownloading && (
          <span className="mono" style={{ fontSize: 10, letterSpacing: ".08em", color: "var(--accent-text)", display: "inline-flex", alignItems: "center", gap: 5 }} data-testid="dash-model-dl">
            <span style={{ width: 6, height: 6, borderRadius: 999, background: "var(--accent)" }}></span>
            {s.modelState === "verifying" ? "local model · verifying" : `local model · ${modelPct}%`}
          </span>
        )}
        {/* HUP-S7.6: the Activity monitor pop-out ("why am I waiting", Stop). */}
        <button
          className="mono"
          data-testid="open-activity-monitor"
          onClick={() => void openPopout("monitor")}
          title="Open the Activity monitor in its own window"
          style={{ fontSize: 10, letterSpacing: ".06em", textTransform: "uppercase", color: "var(--tx-2)", background: "transparent", border: "1px solid var(--line-1)", borderRadius: 999, padding: "3px 9px", cursor: "pointer" }}
        >
          Monitor
        </button>
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
      {/* HUP-S5.1 + S5.6: Hermes's browser controls. Render nothing while the browser is off (the default). */}
      {BRIDGE_MODE === "tauri" && (
        <div style={{ flexShrink: 0 }}>
          <BrowserControls api={tauriBrowserApi} onOpen={() => void openPopout("browser")} onSignInRequest={signInHandler} />
        </div>
      )}
      {pickerOpen && (
        <div style={{ padding: 12, borderBottom: "1px solid var(--line-1)", background: "var(--srf-1)", flexShrink: 0, maxHeight: "45%", overflow: "auto" }}>
          <ModelPicker choices={routerChoices} activeId={store.routerActiveId()} onSelect={onPickModel} />
          {!routerChoices.some((c) => c.source === "cloud") && (
            <span data-testid="model-picker-cloud-hint" style={{ display: "block", marginTop: 8, fontSize: 11.5, color: "var(--tx-3)", lineHeight: 1.5 }}>
              No cloud model is connected. Connect your own key for a vendor in Hermes › Controls › Details › Models and its models are listed here.
            </span>
          )}
        </div>
      )}
      {pickError && (
        <span role="alert" data-testid="model-pick-error" style={{ display: "block", padding: "8px 16px", fontSize: 12, color: "var(--warn)", flexShrink: 0 }}>
          That model was not chosen: {pickError}
        </span>
      )}
      <div
        ref={(el) => { store.chatScrollEl = el; }}
        data-testid="chat-messages"
        onScroll={(e) => { follow.current = shouldFollow(e.currentTarget); }}
        style={{ flex: 1, minHeight: 0, overflow: "auto", padding: 16, display: "flex", flexDirection: "column", gap: 16 }}
      >
        {s.chatMsgs.map((m) => (
          <div key={m.id} style={{ display: "flex", flexDirection: "column", gap: 5 }}>
            <span className="mono" style={{ fontSize: 9.5, letterSpacing: ".13em", textTransform: "uppercase", color: m.who === "You" ? "var(--tx-3)" : "var(--accent-text)" }}>
              {m.who}
            </span>
            <div
              data-testid={m.brief ? "brief-card" : undefined}
              style={{ fontSize: 13.5, lineHeight: 1.6, color: "var(--tx-1)", overflowWrap: "anywhere", ...(m.brief ? { border: "1px solid var(--line-2)", borderRadius: 8, padding: "10px 12px", background: "var(--srf-1)" } : {}) }}
            >
              {/* HUP-S0.4: agent replies render as markdown while streaming too (no reflow jump at the
                  end; finished messages are memoized). Never restyle what the user typed. A div, not a
                  span: markdown is block content. */}
              {m.who !== "You" ? <Markdown text={m.text} /> : <span style={{ whiteSpace: "pre-wrap" }}>{m.text}</span>}
              {m.streaming && <span style={{ display: "inline-block", width: 7, height: 14, background: "var(--accent)", marginLeft: 2, verticalAlign: -2, animation: "ccCaret 1s step-end infinite" }}></span>}
            </div>
            {m.error && (
              <span role="alert" style={{ display: "flex", alignItems: "center", gap: 10, fontSize: 12, color: "var(--danger)" }}>
                <span style={{ flex: 1, minWidth: 0, whiteSpace: "pre-line" }}>The agent didn't finish this reply: {m.error}</span>
                {m.toolchainHelp && (
                  <button className="btn btn-sm" data-testid="open-toolchain-settings" onClick={() => store.openToolchainSettings()}>
                    Open toolchain settings
                  </button>
                )}
                <button className="btn btn-sm" onClick={() => void store.retryChat(m.id)}>
                  Retry
                </button>
              </span>
            )}
            {/* SCL-S7.5a: the local model is not running; ask before this message goes anywhere. */}
            {s.chatRouteHold && s.chatRouteHold.msgId === m.id && (
              <LocalModelAsk phase={s.chatRouteHold.phase} options={s.chatRouteHold.options} onChoose={(c) => store.answerLocalModelAsk(c)} />
            )}
            {/* HUP-S2.9: the files the agent changed in this reply, each with Undo. */}
            {undo.cards
              .filter((c) => c.msgId === m.id)
              .map((c) => (
                <FileChangeCard
                  key={c.session + ":" + c.seq}
                  card={c}
                  onUndo={(session, seq) => void undoChange(bridge.agentHarness, session, seq)}
                  onDiff={BRIDGE_MODE === "tauri" ? (session, seq) => void openDiff(session, seq) : undefined}
                />
              ))}
            {/* HUP-S3.3: a saved brief's workflow runs from its card (sidecar loop; its checks decide). */}
            {m.brief && (
              <span style={{ display: "flex", gap: 8, alignItems: "center", flexWrap: "wrap" }}>
                <button
                  className="btn btn-sm btn-secondary"
                  data-testid="brief-run-workflow"
                  disabled={chatBusy}
                  onClick={() => void store.runBriefWorkflow(m.brief!)}
                  title="Run this brief's workflow; only each step's checks decide the result"
                >
                  Run {m.brief.workflow}
                </button>
              </span>
            )}
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
      {/* rc.5 FIX 4: suggestions and the plan card share one bounded area that scrolls on its own,
          so a tall card can never push the composer out of the pane. */}
      <div data-testid="chat-extras" style={{ flexShrink: 0, maxHeight: "45%", overflow: "auto" }}>
      {showSuggestions && (
        <div style={{ display: "flex", gap: 8, flexWrap: "wrap", padding: "0 16px 10px" }}>
          {SUGGESTIONS.map((label) => (
            <button
              key={label}
              onClick={() => void send(label)}
              style={{ fontFamily: "var(--font-sans)", fontSize: 12, color: "var(--tx-2)", background: "var(--srf-1)", border: "1px solid var(--line-1)", borderRadius: 999, padding: "5px 12px", cursor: "pointer" }}
            >
              {label}
            </button>
          ))}
        </div>
      )}
      {planGoal !== null && (
        <div style={{ padding: "0 16px 10px" }}>
          <InterviewCard
            key={planGoal}
            goal={planGoal}
            api={bridge.agentHarness}
            onAccept={(brief, markdown) => {
              store.acceptBrief(brief, markdown);
              setPlanGoal(null);
              setBuildAsk(false);
              if (store.chatInputEl) store.chatInputEl.value = "";
              fitInput();
            }}
            onCancel={() => setPlanGoal(null)}
          />
        </div>
      )}
      </div>
      {/* 0.5.4: the text area spans the composer's full width on its own row, and grows with its
          content; the compact icon actions and Send sit on the row below, so they never narrow it. */}
      <div data-testid="chat-composer" style={{ display: "flex", flexDirection: "column", gap: 8, padding: "12px 16px", borderTop: "1px solid var(--line-1)", flexShrink: 0 }}>
        <div data-testid="chat-input-row" style={{ display: "flex", width: "100%" }}>
          <textarea
            ref={(el) => { store.chatInputEl = el; }}
            data-testid="chat-input"
            className="input"
            rows={1}
            aria-label="Message Hermes"
            placeholder={micOn ? (micInterim || "Listening…") : "Ask about your node, wallet, or memory…"}
            onChange={(e) => {
              setBuildAsk(looksLikeBuildAsk(e.target.value));
              fitInput();
            }}
            onKeyDown={(e) => {
              // Enter sends; Shift+Enter starts a new line (and an IME composition is never sent).
              if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
                e.preventDefault();
                setBuildAsk(false);
                void send((e.target as HTMLTextAreaElement).value);
              }
            }}
            style={{ flex: 1, minWidth: 0, width: "100%", minHeight: 36, maxHeight: inputMax, overflowY: "hidden" }}
          />
        </div>
        <div data-testid="chat-composer-actions" style={{ display: "flex", alignItems: "center", gap: 6 }}>
          <button
            type="button"
            className={"btn btn-icon " + (micOn ? "btn-secondary" : "btn-ghost")}
            data-testid="chat-mic"
            onClick={toggleMic}
            aria-label={micOn ? "Stop dictation" : "Dictate (on-device)"}
            data-tip={micOn ? "Stop dictation" : "Dictate (on-device)"}
            aria-pressed={micOn}
            style={micOn ? { color: "var(--ok)", borderColor: "var(--ok)" } : undefined}
          >
            <MicGlyph />
          </button>
          <button
            type="button"
            className={"btn btn-icon " + (buildAsk ? "btn-secondary" : "btn-ghost")}
            data-testid="plan-it-first"
            onClick={openPlan}
            aria-expanded={planGoal !== null}
            aria-label="Plan it first"
            data-tip="Plan it first: answer a few questions and edit a brief before Hermes builds anything"
          >
            <PlanGlyph />
          </button>
          <span style={{ flex: 1 }}></span>
          <span className="mono" aria-hidden style={{ fontSize: 9.5, letterSpacing: ".06em", color: "var(--tx-3)", whiteSpace: "nowrap" }}>
            Shift+Enter for a new line
          </span>
          <button className="btn btn-primary" onClick={onSend} disabled={chatBusy}>
            Send
          </button>
        </div>
      </div>
    </div>
  );
}
