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
import { ChatHistoryPanel } from "./ChatHistoryPanel";
import { loadChatHistory } from "../shell/slices/chatHistory";
import { composerMax, fitComposer, shouldFollow, useFitToViewport } from "./chatLayout";
import { RunReceipts, RunTimeline } from "./RunTimeline";
import { PolicyProposalCard, SETTINGS_WORK_ROUTE } from "./PolicyProposalCard";
import { contextRows, contextSummary } from "../agent/hermesContext";
import { datasetAttachment, groupAttachment, journalAttachment, matchAttachments, mentionAt, MAX_ATTACHMENTS, type ChatAttachment } from "../agent/chatAttachments";
import { ChatCommandBar, chatCommands, exactCommand, slashMatches, type ChatCommand } from "./ChatCommandBar";
import { failureCopy, hermesDownCopy, type FailureAction } from "../agent/chatFailure";
import { stepAnnouncement } from "../agent/runTimeline";
import { useAuthority } from "../fl/authority";
import { HicBadge } from "../fl/primitives";
import { startRuntime } from "../shell/slices/agent";
import { isMacPlatform } from "../shell/layoutShortcuts";

/** FL design board 01: the new-chat starters. Each names what Hermes really does with it. Never
 *  "earn" (owner Q7): the GPU starter is about rounds and the member's terms. */
export const STARTERS: { t: string; d: string }[] = [
  { t: "Put my GPU to work on rounds at night.", d: "Checks this machine and the network's rounds" },
  { t: "Can this machine join group rounds?", d: "Reads the round coordinator and this device" },
  { t: "What is my staking position?", d: "Reads ValidatorRegistry on this node" },
  { t: "Network status", d: "Reads this node's height and peers" },
];

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
function AttachGlyph() {
  return (
    <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" aria-hidden focusable="false">
      <path d="M20 11.5 L12.5 19 A5 5 0 0 1 5.5 12 L13 4.5 A3.3 3.3 0 0 1 17.7 9.2 L10.3 16.6 A1.6 1.6 0 0 1 8 14.3 L15 7.3"></path>
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

/** Whether an Esc press is for this chat: the pane is on screen, no dialog is open, and focus is
 *  inside the pane or on nothing in particular. */
export function escTargetsChat(pane: HTMLElement | null): boolean {
  if (!pane || !pane.isConnected || typeof document === "undefined") return false;
  if (document.visibilityState === "hidden") return false;
  if (pane.closest("[hidden],[aria-hidden='true']")) return false;
  if (document.querySelector("[role='dialog'],[role='alertdialog'],[aria-modal='true'],dialog[open]")) return false;
  const a = document.activeElement;
  return !a || a === document.body || pane.contains(a);
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
  // 0.5.4: prior conversations and projects (list, search, reopen) — ChatHistoryPanel.
  const [historyOpen, setHistoryOpen] = useState(false);
  // The sealed history opens once per session (and moves any old plaintext entry into it).
  useEffect(() => {
    void loadChatHistory();
  }, []);
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
  // #690: what the member attached with @ (or the attach button) to the message being typed.
  const [attached, setAttached] = useState<ChatAttachment[]>([]);
  const [mention, setMention] = useState<{ query: string } | null>(null);
  const [mentionItems, setMentionItems] = useState<ChatAttachment[] | null>(null);
  const [mentionErrors, setMentionErrors] = useState<string[]>([]);
  const send = (text: string) => {
    follow.current = true;
    const att = attached;
    setAttached([]);
    return store.sendChat(text, att.length ? { attachments: att } : {});
  };
  // The items are read when the menu opens, each from its real source; a failed read is said.
  const openMention = (query: string) => {
    setMention({ query });
    if (mentionItems !== null) return;
    void (async () => {
      const items: ChatAttachment[] = [];
      const errs: string[] = [];
      const [ds, gs] = await Promise.allSettled([bridge.flRounds.datasetsList(), bridge.groups.list()]);
      if (ds.status === "fulfilled") items.push(...ds.value.map(datasetAttachment));
      else errs.push("Datasets could not be read: " + (ds.reason instanceof Error ? ds.reason.message : String(ds.reason)));
      if (gs.status === "fulfilled") items.push(...gs.value.map(groupAttachment));
      else errs.push("Groups could not be read: " + (gs.reason instanceof Error ? gs.reason.message : String(gs.reason)));
      items.push(...(store.state.jPages || []).map(journalAttachment));
      setMentionItems(items);
      setMentionErrors(errs);
    })();
  };
  const pickAttachment = (a: ChatAttachment) => {
    setAttached((cur) => (cur.some((x) => x.id === a.id) || cur.length >= MAX_ATTACHMENTS ? cur : [...cur, a]));
    const el = store.chatInputEl;
    if (el) {
      const at = mentionAt(el.value);
      if (at) el.value = el.value.slice(0, at.start).replace(/\s$/, "") + (at.start > 0 ? " " : "");
      el.focus();
    }
    setMention(null);
  };

  // Board 01: editing a past message branches the chat (the original stays in History).
  const [editing, setEditing] = useState<string | null>(null);
  const startEdit = (id: string) => {
    const m = s.chatMsgs.find((x) => x.id === id);
    const el = store.chatInputEl;
    if (!m || !el) return;
    setEditing(id);
    el.value = m.text;
    fitComposer(el, inputMax);
    el.focus();
  };
  const cancelEdit = () => {
    setEditing(null);
    if (store.chatInputEl) store.chatInputEl.value = "";
    fitInput();
  };
  const copyText = (t: string) => {
    try {
      void navigator.clipboard.writeText(t).then(
        () => store.toast("Copied"),
        () => store.toast("Copying is not available here"),
      );
    } catch {
      store.toast("Copying is not available here");
    }
  };
  // Board 01: retry an answer on another model, as a branch (the original stays in History).
  const [retryPick, setRetryPick] = useState<string | null>(null);
  const retryWith = async (answerId: string, modelId: string) => {
    setRetryPick(null);
    const chosen = routerChoices.find((c) => c.id === modelId);
    const err = await store.selectModel(modelId, routerChoices);
    if (err) {
      setPickError(err);
      return;
    }
    if (chosen?.source === "local") await sliceSelectModel(modelId);
    await store.retryAsBranch(answerId);
  };
  const [contextOpen, setContextOpen] = useState(false);
  const ctxInput = {
    toolNames: store.chatToolNames(),
    providerKind: store.activeProviderKind(),
    hermesDown: !!s.chatHermesDown,
    modelLabel: activeModel.label,
    off: s.chatContextOff,
    datasetsAttached: new Set(s.chatMsgs.flatMap((m) => (m.attachments ?? []).filter((a) => a.kind === "dataset").map((a) => a.id))).size,
  };

  // Board 01: typing while a run is live offers Steer now or Queue for after (the text waits here).
  const [followUp, setFollowUp] = useState<string | null>(null);
  const runLive = chatBusy;
  const submit = (raw: string) => {
    const text = raw.trim();
    if (!text) return;
    setBuildAsk(false);
    setSlashOpen(false);
    if (runLive) {
      setFollowUp(text);
      return;
    }
    if (editing) {
      const id = editing;
      setEditing(null);
      follow.current = true;
      void store.branchChat(id, { text });
      return;
    }
    void send(text);
  };
  const onSend = () => submit(store.chatInputEl ? store.chatInputEl.value : "");
  const steer = () => {
    if (followUp === null) return;
    void store.steerChat(followUp);
    setFollowUp(null);
  };
  const queue = () => {
    if (followUp === null) return;
    store.queueChat(followUp, attached);
    setAttached([]);
    setFollowUp(null);
  };
  useEffect(() => {
    if (!runLive && followUp !== null) {
      // The run ended before a choice: the text goes back in the composer, unsent.
      const el = store.chatInputEl;
      if (el && !el.value.trim()) el.value = followUp;
      setFollowUp(null);
    }
  }, [runLive, followUp, store]);

  // Board 01: Esc stops a live run (not while an approval sheet or the command bar is open: Esc
  // declines or closes those). Cmd K (Ctrl K) opens the chat's command bar.
  const [barOpen, setBarOpen] = useState(false);
  const [slashOpen, setSlashOpen] = useState(false);
  const [slashText, setSlashText] = useState("");
  const sheetOpen = s.queue.length > 0 || s.walletReview !== null;
  useEffect(() => {
    const mac = isMacPlatform();
    const onKey = (e: KeyboardEvent) => {
      const mod = mac ? e.metaKey && !e.ctrlKey : e.ctrlKey && !e.metaKey;
      if (mod && !e.altKey && !e.shiftKey && (e.code === "KeyK" || e.key === "k" || e.key === "K")) {
        e.preventDefault();
        setBarOpen((v) => !v);
        return;
      }
      // Esc stops only this chat's run: not a key repeat, no dialog open anywhere, this chat on
      // screen, and focus in the chat (or nowhere in particular).
      if (e.key === "Escape" && !e.repeat && !barOpen && !sheetOpen && runLive && escTargetsChat(paneRef.current)) {
        e.preventDefault();
        store.stopAgentTurn();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [barOpen, sheetOpen, runLive, store]);

  const authority = useAuthority();
  const down = s.chatHermesDown;
  const [starting, setStarting] = useState(false);
  const startHermes = async () => {
    setStarting(true);
    const err = await startRuntime("hermes");
    setStarting(false);
    if (err) store.toast("Hermes did not start: " + err);
  };
  const runAction = (a: FailureAction, msgId: string) => {
    if (a === "retry") void store.retryChat(msgId);
    else if (a === "pick_model") setPickerOpen(true);
    else if (a === "start_hermes") void startHermes();
    else store.go("hermes/controls");
  };
  const ACTION_WORD: Record<FailureAction, string> = { retry: "Retry", pick_model: "Pick a model", start_hermes: "Start Hermes", open_controls: "Open Hermes › Controls" };
  const commands = chatCommands({
    live: runLive,
    stop: () => store.stopAgentTurn(),
    newChat: () => {
      if (store.newChat()) setHistoryOpen(false);
    },
    history: () => setHistoryOpen(true),
    model: () => setPickerOpen(true),
    plan: () => openPlan(),
    monitor: () => void openPopout("monitor"),
    focus: () => store.chatInputEl?.focus(),
    context: () => setContextOpen(true),
  });
  const runCommand = (c: ChatCommand) => {
    setBarOpen(false);
    setSlashOpen(false);
    if (c.insert !== undefined) {
      const el = store.chatInputEl;
      if (el) {
        el.value = c.insert;
        el.focus();
      }
      return;
    }
    if (store.chatInputEl && store.chatInputEl.value.trim().startsWith("/")) store.chatInputEl.value = "";
    c.run();
  };
  // The latest live run, for the step announcement (spoken once per step, never per token).
  const liveRun = [...s.chatMsgs].reverse().find((m) => m.run && m.run.outcome === null)?.run;
  const announce = stepAnnouncement(liveRun ?? [...s.chatMsgs].reverse().find((m) => m.run)?.run);
  const streamingNow = s.chatMsgs.some((m) => m.streaming);
  // Only this conversation's queued follow-ups (they are only ever sent here).
  const queued = s.chatQueue.filter((q) => q.threadId === s.chatThreadId);

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
      <div style={{ display: "flex", alignItems: "center", flexWrap: "wrap", gap: 10, rowGap: 6, padding: "12px 16px", borderBottom: "1px solid var(--line-1)", flexShrink: 0 }}>
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
        <button
          type="button"
          className="mono"
          data-testid="open-command-bar"
          aria-haspopup="dialog"
          aria-expanded={barOpen}
          aria-keyshortcuts={isMacPlatform() ? "Meta+K" : "Control+K"}
          onClick={() => setBarOpen(true)}
          title="Commands for this chat"
          style={{ fontSize: 10, letterSpacing: ".06em", textTransform: "uppercase", color: "var(--tx-2)", background: "transparent", border: "1px solid var(--line-1)", borderRadius: 999, padding: "3px 9px", cursor: "pointer" }}
        >
          {isMacPlatform() ? "⌘K" : "Ctrl K"}
        </button>
        <button
          type="button"
          className="mono"
          data-testid="open-context"
          aria-expanded={contextOpen}
          aria-controls="hermes-context"
          onClick={() => {
            setContextOpen((v) => !v);
            setHistoryOpen(false);
            setPickerOpen(false);
          }}
          title="What Hermes can see in this chat"
          style={{ fontSize: 10, letterSpacing: ".06em", textTransform: "uppercase", color: "var(--tx-2)", background: contextOpen ? "var(--srf-2)" : "transparent", border: "1px solid var(--line-1)", borderRadius: 999, padding: "3px 9px", cursor: "pointer" }}
        >
          Can see
        </button>
        {/* 0.5.4: the way back to earlier conversations and projects, and a fresh conversation. */}
        <button
          type="button"
          className="mono"
          data-testid="open-chat-history"
          aria-expanded={historyOpen}
          aria-controls="chat-history"
          onClick={() => {
            setHistoryOpen((v) => !v);
            setPickerOpen(false);
          }}
          title="Earlier conversations and projects on this device"
          style={{ fontSize: 10, letterSpacing: ".06em", textTransform: "uppercase", color: "var(--tx-2)", background: historyOpen ? "var(--srf-2)" : "transparent", border: "1px solid var(--line-1)", borderRadius: 999, padding: "3px 9px", cursor: "pointer" }}
        >
          History
        </button>
        <button
          type="button"
          className="mono"
          data-testid="new-chat"
          disabled={chatBusy}
          onClick={() => {
            if (store.newChat()) setHistoryOpen(false);
          }}
          title="Start a new conversation (this one stays in History)"
          style={{ fontSize: 10, letterSpacing: ".06em", textTransform: "uppercase", color: "var(--tx-2)", background: "transparent", border: "1px solid var(--line-1)", borderRadius: 999, padding: "3px 9px", cursor: chatBusy ? "not-allowed" : "pointer" }}
        >
          New chat
        </button>
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
          onClick={() => {
            setPickerOpen((v) => !v);
            setHistoryOpen(false);
          }}
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
      {contextOpen && (
        <section id="hermes-context" data-testid="hermes-context" aria-labelledby="hermes-context-h" style={{ padding: 12, borderBottom: "1px solid var(--line-1)", background: "var(--srf-1)", flexShrink: 0, maxHeight: "45%", overflow: "auto", display: "flex", flexDirection: "column", gap: 8 }}>
          <h3 id="hermes-context-h" className="eyebrow" style={{ margin: 0, fontSize: 10 }}>
            What Hermes can see here
          </h3>
          <span style={{ fontSize: 12, color: "var(--tx-2)" }}>{contextSummary(ctxInput)}</span>
          <ul style={{ listStyle: "none", margin: 0, padding: 0, display: "flex", flexDirection: "column", gap: 6 }}>
            {contextRows(ctxInput).map((r) => (
              <li key={r.id} data-testid="context-row" data-can={String(r.can)} style={{ display: "grid", gridTemplateColumns: "40px minmax(0,1fr)", gap: 8, fontSize: 12.5, alignItems: "start" }}>
                {r.toggle ? (
                  <input
                    type="checkbox"
                    data-testid={"context-toggle-" + r.id}
                    id={"ctx-" + r.id}
                    checked={!r.off}
                    onChange={(e) => store.setChatContext(r.id, e.target.checked)}
                    style={{ marginTop: 2, accentColor: "var(--ok)" }}
                  />
                ) : (
                  <span className="mono" style={{ fontSize: 10.5, color: r.can ? "var(--ok)" : "var(--tx-3)" }}>
                    {r.can ? "Yes" : "No"}
                  </span>
                )}
                <label htmlFor={r.toggle ? "ctx-" + r.id : undefined} style={{ display: "flex", flexDirection: "column", gap: 1, cursor: r.toggle ? "pointer" : "default" }}>
                  <span>{r.title}</span>
                  <span style={{ fontSize: 11, color: "var(--tx-3)" }}>{r.detail}</span>
                </label>
              </li>
            ))}
          </ul>
        </section>
      )}
      {historyOpen && (
        <div style={{ padding: 12, borderBottom: "1px solid var(--line-1)", background: "var(--srf-1)", flexShrink: 0, maxHeight: "45%", overflow: "auto" }}>
          <ChatHistoryPanel store={store} currentId={s.chatThreadId} busy={chatBusy} onOpened={() => setHistoryOpen(false)} />
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
      {barOpen && <ChatCommandBar commands={commands} onRun={runCommand} onClose={() => setBarOpen(false)} />}
      {down && (() => {
        const c = hermesDownCopy(down);
        return (
          <div role="alert" data-testid="hermes-down" data-why={down.why} style={{ display: "flex", gap: 12, alignItems: "center", flexWrap: "wrap", margin: "10px 16px 0", padding: "10px 14px", border: "1px solid var(--danger)", background: "var(--danger-bg)", borderRadius: 8, flexShrink: 0 }}>
            <span style={{ flex: 1, minWidth: 220, display: "flex", flexDirection: "column", gap: 3 }}>
              <span style={{ fontWeight: 600, color: "var(--danger)", fontSize: 13 }}>{c.title}</span>
              <span style={{ fontSize: 12.5, color: "var(--tx-1)", lineHeight: 1.5 }}>{c.sentence}</span>
            </span>
            {c.action === "start_hermes" && (
              <button type="button" className="btn btn-primary btn-sm" data-testid="start-hermes" disabled={starting} onClick={() => void startHermes()}>
                {starting ? "Starting Hermes" : "Start Hermes"}
              </button>
            )}
            {c.action === "open_settings" && (
              <button type="button" className="btn btn-sm" onClick={() => store.go("hermes/controls")}>
                Turn the loop on in Hermes › Controls
              </button>
            )}
            {c.action === "open_controls" && (
              <button type="button" className="btn btn-sm" onClick={() => store.go("hermes/controls")}>
                Open Hermes › Controls
              </button>
            )}
          </div>
        );
      })()}
      {/* Board 01 a11y: each step change is spoken once (the text only changes with the step). */}
      <div role="status" aria-live="polite" className="sr-only" data-testid="run-announce">
        {announce}
      </div>
      <div
        ref={(el) => { store.chatScrollEl = el; }}
        data-testid="chat-messages"
        role="log"
        aria-live="polite"
        aria-busy={streamingNow}
        aria-label="Conversation with Hermes"
        onScroll={(e) => { follow.current = shouldFollow(e.currentTarget); }}
        style={{ flex: 1, minHeight: 0, overflow: "auto", padding: 16, display: "flex", flexDirection: "column", gap: 16 }}
      >
        {showSuggestions && (
          <div data-testid="chat-empty" style={{ display: "flex", flexDirection: "column", gap: 10, padding: "12px 0 4px" }}>
            <h2 style={{ margin: 0, fontSize: 22, fontWeight: 500, color: "var(--tx-1)" }}>What should Hermes do on this machine?</h2>
            <p style={{ margin: 0, fontSize: 13, lineHeight: 1.55, color: "var(--tx-2)" }}>
              Hermes runs on your device. It reads what you allow, drafts changes, and asks before anything is signed, spent or joined.
            </p>
          </div>
        )}
        {s.chatMsgs.map((m) => (
          <div key={m.id} data-testid={m.who === "You" ? "msg-you" : "msg-agent"} style={{ display: "flex", flexDirection: "column", gap: 5 }}>
            <span className="mono" style={{ fontSize: 9.5, letterSpacing: ".13em", textTransform: "uppercase", color: m.who === "You" ? "var(--tx-3)" : "var(--accent-text)" }}>
              {m.who}
              {m.steer && (
                <span data-testid="steer-label" style={{ color: "var(--tx-3)", letterSpacing: ".04em", textTransform: "none" }}>
                  {m.steer === "taken" ? " · steer, taken mid-run" : " · steer, waiting for the next step"}
                </span>
              )}
              {m.model && m.who !== "You" && <span style={{ color: "var(--tx-3)", letterSpacing: ".04em", textTransform: "none" }}> · {m.model}</span>}
            </span>
            {m.run && <RunTimeline run={m.run} onStop={m.run.outcome === null ? () => store.stopAgentTurn() : undefined} />}
            {!(m.run && m.streaming && m.text === "") && <div
              data-testid={m.brief ? "brief-card" : undefined}
              style={{ fontSize: 13.5, lineHeight: 1.6, color: "var(--tx-1)", overflowWrap: "anywhere", ...(m.brief ? { border: "1px solid var(--line-2)", borderRadius: 8, padding: "10px 12px", background: "var(--srf-1)" } : {}) }}
            >
              {/* HUP-S0.4: agent replies render as markdown while streaming too (no reflow jump at the
                  end; finished messages are memoized). Never restyle what the user typed. A div, not a
                  span: markdown is block content. */}
              {m.who !== "You" ? <Markdown text={m.text} /> : <span style={{ whiteSpace: "pre-wrap" }}>{m.text}</span>}
              {m.attachments && m.attachments.length > 0 && (
                <span data-testid="msg-attachments" style={{ display: "flex", gap: 6, flexWrap: "wrap", marginTop: 4 }}>
                  {m.attachments.map((a) => (
                    <span key={a.id} className="fl-chip" title={a.context}>
                      {a.label}
                    </span>
                  ))}
                </span>
              )}
              {m.streaming && <span style={{ display: "inline-block", width: 7, height: 14, background: "var(--accent)", marginLeft: 2, verticalAlign: -2, animation: "ccCaret 1s step-end infinite" }}></span>}
            </div>}
            {m.error && (() => {
              // Board 01 "Honest failure": what failed in one sentence, and the fix.
              const f = failureCopy(m.error);
              return (
                <span role="alert" data-testid="chat-failure" data-kind={f.kind} style={{ display: "flex", alignItems: "center", gap: 10, fontSize: 12, color: f.kind === "stopped" ? "var(--tx-2)" : "var(--danger)", flexWrap: "wrap" }}>
                  <span style={{ flex: 1, minWidth: 200, whiteSpace: "pre-line" }}>
                    {f.sentence} <span style={{ color: "var(--tx-2)" }}>{f.fix}</span>
                  </span>
                  {m.toolchainHelp && (
                    <button className="btn btn-sm" data-testid="open-toolchain-settings" onClick={() => store.openToolchainSettings()}>
                      Open toolchain settings
                    </button>
                  )}
                  {f.actions.map((a) => (
                    <button key={a} type="button" className="btn btn-sm" data-testid={"failure-" + a} disabled={a === "retry" && chatBusy} onClick={() => runAction(a, m.id)}>
                      {ACTION_WORD[a]}
                    </button>
                  ))}
                </span>
              );
            })()}
            {m.proposal && (
              <PolicyProposalCard
                proposal={m.proposal}
                onConfirm={() => void store.confirmProposal(m.proposal!.id)}
                onDecline={() => store.declineProposal(m.proposal!.id)}
                onReopen={() => store.reopenProposal(m.proposal!.id)}
                onEdit={() => store.go(SETTINGS_WORK_ROUTE)}
              />
            )}
            {m.who === "You" && !chatBusy && m.text.trim() !== "" && (
              <span style={{ display: "flex", gap: 2, alignSelf: "flex-start" }}>
                <button type="button" className="btn btn-ghost btn-sm" data-testid="msg-edit" onClick={() => startEdit(m.id)} title="Edit to branch; the original stays in History">
                  Edit
                </button>
                <button type="button" className="btn btn-ghost btn-sm" onClick={() => copyText(m.text)}>
                  Copy
                </button>
              </span>
            )}
            {m.who !== "You" && !m.streaming && !m.error && m.text.trim() !== "" && (
              <span style={{ display: "flex", gap: 2, flexWrap: "wrap", alignItems: "center" }}>
                <button type="button" className="btn btn-ghost btn-sm" onClick={() => copyText(m.text)}>
                  Copy
                </button>
                <button type="button" className="btn btn-ghost btn-sm" data-testid="msg-retry" disabled={chatBusy} onClick={() => void store.retryAsBranch(m.id)} title="Ask again in a new branch; this answer stays in History">
                  Retry
                </button>
                <button type="button" className="btn btn-ghost btn-sm" data-testid="msg-retry-with" disabled={chatBusy} aria-expanded={retryPick === m.id} onClick={() => setRetryPick((v) => (v === m.id ? null : m.id))}>
                  Retry with another model
                </button>
                <button type="button" className="btn btn-ghost btn-sm" data-testid="msg-branch" disabled={chatBusy} onClick={() => void store.branchChat(m.id, { includeFrom: true })}>
                  Branch from here
                </button>
              </span>
            )}
            {retryPick === m.id && (
              <ul data-testid="retry-models" aria-label="Retry with" style={{ listStyle: "none", margin: 0, padding: 4, border: "1px solid var(--line-1)", borderRadius: 6, display: "flex", flexDirection: "column", gap: 2 }}>
                {routerChoices
                  .filter((c) => c.ready && c.id !== store.routerActiveId())
                  .map((c) => (
                    <li key={c.id}>
                      <button type="button" className="btn btn-ghost btn-sm" data-testid="retry-model" style={{ width: "100%", justifyContent: "flex-start" }} onClick={() => void retryWith(m.id, c.id)}>
                        {c.label}
                      </button>
                    </li>
                  ))}
                {routerChoices.filter((c) => c.ready && c.id !== store.routerActiveId()).length === 0 && (
                  <li style={{ fontSize: 12, color: "var(--tx-3)", padding: "4px 8px" }}>No other model is ready. Connect one in Hermes › Controls › Details › Models.</li>
                )}
              </ul>
            )}
            {m.who !== "You" && !m.streaming && <RunReceipts run={m.run} />}
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
        {chatThinking && !liveRun && (
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
        <div data-testid="chat-starters" style={{ display: "grid", gridTemplateColumns: "repeat(auto-fill, minmax(220px, 1fr))", gap: 8, padding: "0 16px 10px" }}>
          {STARTERS.map((x) => (
            <button
              key={x.t}
              type="button"
              data-testid="chat-starter"
              onClick={() => void send(x.t)}
              style={{ textAlign: "left", font: "inherit", display: "flex", flexDirection: "column", gap: 2, color: "var(--tx-1)", background: "var(--srf-1)", border: "1px solid var(--line-1)", borderRadius: 8, padding: "9px 12px", cursor: "pointer", minHeight: 36 }}
            >
              <span style={{ fontSize: 13, fontWeight: 500 }}>{x.t}</span>
              <span style={{ fontSize: 11.5, color: "var(--tx-2)" }}>{x.d}</span>
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
      {editing && (
        <div data-testid="editing-banner" role="status" style={{ display: "flex", alignItems: "center", gap: 8, margin: "0 16px 6px", padding: "6px 10px", border: "1px solid var(--line-1)", borderRadius: 6, fontSize: 12.5, flexShrink: 0 }}>
          <span style={{ flex: 1 }}>Editing a past message starts a branch. The original stays in History.</span>
          <button type="button" className="btn btn-ghost btn-sm" onClick={cancelEdit}>
            Cancel
          </button>
        </div>
      )}
      {followUp !== null && runLive && (
        <div data-testid="steer-offer" role="group" aria-label="Your next message" style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap", margin: "0 16px 6px", padding: "6px 10px", background: "var(--srf-inset, var(--srf-1))", border: "1px solid var(--line-1)", borderRadius: 6, fontSize: 12.5, flexShrink: 0 }}>
          <span style={{ width: 7, height: 7, borderRadius: 999, background: "var(--warn)" }} aria-hidden></span>
          <span style={{ flex: 1, minWidth: 180 }}>
            Hermes is working. Your next message: <i>{followUp}</i>
          </span>
          <button type="button" className="btn btn-sm btn-primary" data-testid="steer-now" onClick={steer} title="Hermes takes your message at its next step and keeps working. If it cannot, this run stops and your message goes next.">
            Steer now
          </button>
          <button type="button" className="btn btn-sm" data-testid="queue-after" onClick={queue} title="Sends when this run finishes">
            Queue for after
          </button>
        </div>
      )}
      {queued.length > 0 && (
        <div data-testid="chat-queue" style={{ display: "flex", flexDirection: "column", gap: 4, margin: "0 16px 6px", flexShrink: 0 }}>
          <span className="mono" style={{ fontSize: 9.5, letterSpacing: ".1em", textTransform: "uppercase", color: "var(--tx-3)" }}>
            Queued, sends after this run
          </span>
          <ol style={{ listStyle: "none", margin: 0, padding: 0, display: "flex", flexDirection: "column", gap: 4 }}>
            {queued.map((q) => (
              <li key={q.id} data-testid="queued-msg" style={{ display: "flex", alignItems: "center", gap: 8, fontSize: 12.5, padding: "4px 8px", border: "1px dashed var(--line-2)", borderRadius: 6 }}>
                <span style={{ flex: 1, minWidth: 0, overflowWrap: "anywhere" }}>{q.text}</span>
                {q.steer && <span className="mono" style={{ fontSize: 10, color: "var(--tx-3)" }}>after the stop</span>}
                {!runLive && !q.steer && (
                  <button type="button" className="btn btn-ghost btn-sm" onClick={() => store.sendQueuedNow(q.id)}>
                    Send now
                  </button>
                )}
                <button type="button" className="btn btn-ghost btn-sm" aria-label={"Remove queued message: " + q.text} onClick={() => store.dropQueuedChat(q.id)}>
                  Remove
                </button>
              </li>
            ))}
          </ol>
        </div>
      )}
      {slashOpen && slashMatches(commands, slashText).length > 0 && (
        <ul data-testid="slash-menu" role="listbox" aria-label="Commands" style={{ listStyle: "none", margin: "0 16px 6px", padding: 4, border: "1px solid var(--line-2)", borderRadius: 6, background: "var(--srf-1)", flexShrink: 0 }}>
          {slashMatches(commands, slashText).map((c) => (
            <li key={c.id} role="option" aria-selected={false}>
              <button type="button" className="btn btn-ghost btn-sm" style={{ width: "100%", justifyContent: "flex-start", gap: 8 }} onClick={() => runCommand(c)}>
                <span className="mono">{c.slash}</span>
                <span style={{ color: "var(--tx-2)" }}>{c.label}</span>
              </button>
            </li>
          ))}
        </ul>
      )}
      {mention && (
        <div data-testid="mention-menu" role="listbox" aria-label="Attach for context" style={{ margin: "0 16px 6px", padding: 4, border: "1px solid var(--line-2)", borderRadius: 6, background: "var(--srf-1)", flexShrink: 0, maxHeight: 220, overflow: "auto" }}>
          {mentionItems === null && <div style={{ padding: "6px 10px", fontSize: 12.5, color: "var(--tx-3)" }}>Reading datasets, groups and journal pages on this device</div>}
          {mentionErrors.map((e) => (
            <div key={e} role="alert" style={{ padding: "4px 10px", fontSize: 12, color: "var(--warn)" }}>
              {e}
            </div>
          ))}
          {mentionItems !== null && matchAttachments(mentionItems, mention.query).length === 0 && (
            <div style={{ padding: "6px 10px", fontSize: 12.5, color: "var(--tx-3)" }}>Nothing on this device matches. Datasets come from Groups › Rounds › Datasets.</div>
          )}
          {mentionItems !== null &&
            matchAttachments(mentionItems, mention.query).map((a) => (
              <button key={a.id} type="button" role="option" aria-selected={false} data-testid="mention-item" className="btn btn-ghost btn-sm" style={{ width: "100%", justifyContent: "flex-start" }} onClick={() => pickAttachment(a)}>
                {a.label}
              </button>
            ))}
        </div>
      )}
      <div data-testid="chat-composer" style={{ display: "flex", flexDirection: "column", gap: 8, padding: "12px 16px", borderTop: "1px solid var(--line-1)", flexShrink: 0 }}>
        {attached.length > 0 && (
          <div data-testid="composer-attachments" style={{ display: "flex", gap: 6, flexWrap: "wrap" }}>
            {attached.map((a) => (
              <button key={a.id} type="button" className="fl-chip" title={a.context} aria-label={"Remove attachment " + a.label} onClick={() => setAttached((cur) => cur.filter((x) => x.id !== a.id))}>
                {a.label} ×
              </button>
            ))}
          </div>
        )}
        <div data-testid="chat-input-row" style={{ display: "flex", width: "100%" }}>
          <textarea
            ref={(el) => { store.chatInputEl = el; }}
            data-testid="chat-input"
            className="input"
            rows={1}
            aria-label="Message Hermes"
            placeholder={micOn ? (micInterim || "Listening…") : runLive ? "Hermes is working. Type to steer or queue a follow-up" : "Ask Hermes, or type / for commands"}
            onChange={(e) => {
              setBuildAsk(looksLikeBuildAsk(e.target.value));
              // Board 01: a leading "/" lists the chat's commands.
              const v = e.target.value;
              // #690: "@" at the start of a word lists what can be attached for context.
              const at = mentionAt(v);
              if (at) openMention(at.query);
              else if (mention) setMention(null);
              setSlashOpen(v.startsWith("/") && !v.includes("\n"));
              setSlashText(v);
              fitInput();
            }}
            onKeyDown={(e) => {
              // Enter (or Cmd/Ctrl Enter) sends; Shift+Enter starts a new line (an IME composition
              // is never sent). While a run is live, the text is offered as a steer or a queued
              // follow-up instead.
              if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
                e.preventDefault();
                const v = (e.target as HTMLTextAreaElement).value;
                if (v.trim().startsWith("/") && !/^\/run\b/.test(v.trim())) {
                  // Enter runs a command only when its exact word was typed (the menu lists prefixes).
                  const hit = exactCommand(commands, v);
                  if (hit) {
                    runCommand(hit);
                    return;
                  }
                }
                submit(v);
              }
              // Board 01: Up in an empty composer edits your last message (as a branch).
              if (e.key === "ArrowUp" && !runLive && !editing && (e.target as HTMLTextAreaElement).value === "") {
                const last = [...s.chatMsgs].reverse().find((x) => x.who === "You" && x.text.trim() !== "");
                if (last) {
                  e.preventDefault();
                  startEdit(last.id);
                }
              }
              if (e.key === "Escape" && editing) {
                e.preventDefault();
                e.stopPropagation();
                cancelEdit();
              }
              if (e.key === "Escape" && mention) {
                e.preventDefault();
                e.stopPropagation();
                setMention(null);
              }
              if (e.key === "Escape" && slashOpen) {
                e.preventDefault();
                e.stopPropagation();
                setSlashOpen(false);
              }
            }}
            aria-keyshortcuts="Enter Meta+Enter Control+Enter"
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
            className="btn btn-icon btn-ghost"
            data-testid="chat-attach"
            aria-label="Attach a dataset, group or journal page"
            data-tip="Attach a dataset, group or journal page (or type @)"
            aria-expanded={mention !== null}
            onClick={() => (mention ? setMention(null) : openMention(""))}
          >
            <AttachGlyph />
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
          {/* Board 01: who may act right now (shared with Settings › Work, src/fl/authority.ts). */}
          <span
            data-testid="authority-badge"
            data-level={authority.level}
            title={authority.level === "HIC-1" ? "HIC-1: Hermes proposes, and every action waits for your decision" : "HIC-2: Hermes may act inside this envelope's caps"}
            style={{ display: "inline-flex", alignItems: "center", gap: 6, fontSize: 11.5, color: "var(--tx-2)", border: "1px solid var(--line-1)", borderRadius: 999, padding: "2px 8px 2px 3px", whiteSpace: "nowrap", maxWidth: "50%", overflow: "hidden", textOverflow: "ellipsis" }}
          >
            <HicBadge level={authority.level} />
            {authority.level === "HIC-1"
              ? "Every action asks you"
              : `${authority.name}${authority.caps.length ? " · " + authority.caps.map((c) => `${c.left} ${c.label}`).join(" · ") : ""}`}
          </span>
          <button className="btn btn-primary" data-testid="chat-send" onClick={onSend}>
            Send
          </button>
        </div>
        <div className="mono" aria-hidden style={{ fontSize: 9.5, letterSpacing: ".04em", color: "var(--tx-3)", textAlign: "center" }}>
          Enter send · Shift Enter new line · Esc stop · Up edit last · / commands · @ context · {isMacPlatform() ? "⌘K" : "Ctrl K"} command bar
        </div>
      </div>
    </div>
  );
}
