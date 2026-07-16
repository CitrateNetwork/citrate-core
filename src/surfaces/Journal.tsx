// =====================================================================
// citrate-core — Journal surface (1:1 from design/CitrateCore.dc.html
// JOURNAL section + inline journal derivations/handlers).
//
// LOCAL, off-chain surface. Daily notes + named pages, bullet blocks with
// [[links]], @agent / @prompt tags, backlinks, voice capture, agent chat,
// and encrypted pinning (via the SignatureCeremony). No chain read is
// live here — the honest framing is "local and off-chain, encrypted at
// rest in your data dir" (Rule 1). The single genuinely-live datum in the
// app (chain-40204 height) lives on the Dashboard, not here.
//
// Charter/light surface — no data-register wrapper (App applies register).
//
// Store methods reused (already ported wave-1): appendCapture, exportJournal,
// sendChat, requestSig, toast, save, setState, and the jChatInputEl /
// jChatScrollEl refs. The design's `toggleMic`/`initSpeech` dictation loop
// is NOT on the store, so it is implemented in-file below against the
// jMicOn / jInterim AppState fields (which exist in freshState) via
// store.setState. onJNew / onJEdit / onJPin are inline handlers in the
// design's view derivation, reproduced here 1:1 against existing state.
// =====================================================================
import { useRef } from "react";
import { LoaderMark } from "../components/LoaderMark";
import { SurfaceProps } from "./shared";
import { JournalPage } from "../shell/state";

// ---------- dictation (ported from design initSpeech/toggleMic) ----------
// The design keeps a single SpeechRecognition instance on the logic
// instance; here it is a module-level singleton bound to the store. Speech
// results append to the shared journal chat input (store.jChatInputEl) and
// interim text streams into jInterim, exactly as the prototype.
interface SpeechLike {
  continuous: boolean;
  interimResults: boolean;
  lang: string;
  onresult: ((e: SpeechRecognitionEvent) => void) | null;
  onend: (() => void) | null;
  onerror: ((e: SpeechRecognitionErrorEvent) => void) | null;
  start(): void;
  stop(): void;
}
interface SpeechRecognitionEvent {
  resultIndex: number;
  results: { length: number; [i: number]: { isFinal: boolean; 0: { transcript: string } } };
}
interface SpeechRecognitionErrorEvent {
  error: string;
}
type SpeechWindow = typeof window & {
  SpeechRecognition?: new () => SpeechLike;
  webkitSpeechRecognition?: new () => SpeechLike;
};

// `undefined` = not yet probed, `null` = unavailable in this browser.
let rec: SpeechLike | null | undefined;

function initSpeech(store: SurfaceProps["store"]): SpeechLike | null {
  if (rec !== undefined) return rec;
  const w = window as SpeechWindow;
  const SR = w.SpeechRecognition || w.webkitSpeechRecognition;
  if (!SR) {
    rec = null;
    return null;
  }
  const r = new SR();
  r.continuous = true;
  r.interimResults = true;
  r.lang = "en-US";
  r.onresult = (e) => {
    let interim = "",
      fin = "";
    for (let i = e.resultIndex; i < e.results.length; i++) {
      const t = e.results[i][0].transcript;
      if (e.results[i].isFinal) fin += t;
      else interim += t;
    }
    if (fin && store.jChatInputEl) store.jChatInputEl.value = (store.jChatInputEl.value + " " + fin).trim();
    store.setState({ jInterim: interim });
  };
  r.onend = () => {
    if (store.state.jMicOn) {
      try {
        r.start();
      } catch {
        /* ignore */
      }
    }
  };
  r.onerror = (e) => {
    if (e.error === "not-allowed" || e.error === "service-not-allowed") {
      store.setState({ jMicOn: false, jInterim: "" });
      store.toast("Microphone permission denied — typing still works");
    }
  };
  rec = r;
  return r;
}

function toggleMic(store: SurfaceProps["store"]): void {
  const r = initSpeech(store);
  if (!r) {
    store.toast("Dictation needs Chrome or Edge here — typing works everywhere");
    return;
  }
  if (store.state.jMicOn) {
    store.setState({ jMicOn: false, jInterim: "" });
    try {
      r.stop();
    } catch {
      /* ignore */
    }
  } else {
    store.setState({ jMicOn: true });
    try {
      r.start();
    } catch {
      store.setState({ jMicOn: false });
    }
  }
}

export function Journal({ store, s }: SurfaceProps) {
  // Editor element refs live on the component (the store owns only the chat
  // input/scroll refs). The design keeps jTextEl/jTitleEl on the logic
  // instance; here they are component-local refs since store.ts is frozen.
  const jTextRef = useRef<HTMLTextAreaElement | null>(null);
  const jTitleRef = useRef<HTMLInputElement | null>(null);

  const jPages = s.jPages || [];
  const jSelPage: JournalPage | null = jPages.find((p) => p.id === s.jSel) || jPages[0] || null;

  const jItem = (p: JournalPage) => ({
    id: p.id,
    label: p.title,
    pinned: p.pinned,
    weight: jSelPage && jSelPage.id === p.id ? 500 : 400,
    bg: jSelPage && jSelPage.id === p.id ? "var(--srf-1)" : "transparent",
    color: jSelPage && jSelPage.id === p.id ? "var(--tx-1)" : "var(--tx-2)",
    go: () => {
      store.setState({ jSel: p.id, jEditing: false });
      store.save();
    },
  });

  const jDaily = jPages.filter((p) => p.kind === "daily").sort((a, b) => (a.title < b.title ? 1 : -1)).map(jItem);
  const jNamed = jPages.filter((p) => p.kind !== "daily").map(jItem);
  const jViewing = !s.jEditing;
  const jTitle = jSelPage ? jSelPage.title : "—";
  const jMeta = jSelPage ? (jSelPage.pinned ? "pinned · encrypted snapshot" : "local · off-chain") + " · " + jSelPage.blocks.length + " bullets" : "";
  const jEditLabel = s.jEditing ? "Done" : "Edit";

  // Uncontrolled editor refs (design pattern): the textarea / title input
  // are seeded from the selected page's content only when the page id
  // changes (el.dataset.pg), so live typing is never clobbered by re-render.
  const setJText = (el: HTMLTextAreaElement | null) => {
    jTextRef.current = el;
    if (el && jSelPage && el.dataset.pg !== jSelPage.id) {
      el.dataset.pg = jSelPage.id;
      el.value = jSelPage.blocks.join("\n");
      try {
        el.focus();
      } catch {
        /* ignore */
      }
    }
  };
  const setJTitle = (el: HTMLInputElement | null) => {
    jTitleRef.current = el;
    if (el && jSelPage && el.dataset.pg !== jSelPage.id) {
      el.dataset.pg = jSelPage.id;
      el.value = jSelPage.title;
    }
  };

  const onJEdit = () => {
    if (!jSelPage) return;
    if (!s.jEditing) {
      store.setState({ jEditing: true });
      return;
    }
    const raw = jTextRef.current ? jTextRef.current.value : jSelPage.blocks.join("\n");
    let title = jTitleRef.current ? jTitleRef.current.value.trim() : jSelPage.title;
    if (!title) title = jSelPage.title;
    if (jSelPage.kind === "daily") title = jSelPage.title;
    const blocks = raw.split("\n");
    store.setState({
      jEditing: false,
      jPages: jPages.map((p) => (p.id === jSelPage.id ? { ...p, title, blocks: blocks.filter((b) => b.trim() !== "") } : p)),
    });
    store.save();
    if (jSelPage.pinned) store.toast("Saved — pinned snapshot updates at the next challenge window");
  };

  const onJNew = () => {
    const id = "p-" + Math.random().toString(36).slice(2, 8);
    store.setState({ jPages: jPages.concat([{ id, title: "Untitled", kind: "page", pinned: false, blocks: [""] }]), jSel: id, jEditing: true });
  };

  const jPinDisabled = !jSelPage || jSelPage.pinned || s.jEditing;
  const jPinLabel = jSelPage && jSelPage.pinned ? "Pinned" : "Pin securely";
  // Pinning is NOT wired to a real pin daemon / bond tx yet — the old path
  // fabricated a CID by string concat and faked a bond. Be honest (Rule 1/3)
  // until the pinning daemon + grounded bond contract land.
  const onJPin = () => {
    if (!jSelPage || jSelPage.pinned) return;
    store.settleUnwired("Pinning");
  };

  // backlinks: pages that reference [[This page's title]]
  const jBacklinkTag = jSelPage ? "[[" + jSelPage.title + "]]" : null;
  const jBacklinks = jSelPage
    ? jPages
        .filter((p) => p.id !== jSelPage.id && jBacklinkTag !== null && p.blocks.some((b) => b.indexOf(jBacklinkTag) >= 0))
        .map((p) => ({
          id: p.id,
          title: p.title,
          snippet: (p.blocks.find((b) => jBacklinkTag !== null && b.indexOf(jBacklinkTag) >= 0) || "").trim().replace(/^@agent\s+/, ""),
          go: () => {
            store.setState({ jSel: p.id, jEditing: false });
            store.save();
          },
        }))
    : [];
  const jBacklinksShow = jBacklinks.length > 0;

  // bullet blocks: leading-space indent, @agent / @prompt tags, runnable prompts
  const jBlocks = jSelPage
    ? jSelPage.blocks.map((b, i) => {
        const lead = (b.match(/^\s*/) || [""])[0].length;
        let text = b.trim();
        const agent = text.indexOf("@agent ") === 0;
        if (agent) text = text.slice(7);
        const prompt = text.indexOf("@prompt ") === 0;
        if (prompt) text = text.slice(8);
        const ptext = text;
        return {
          key: i,
          text,
          agent,
          prompt,
          pad: 0 + Math.min(3, Math.floor(lead / 2)) * 20 + "px",
          run: () => {
            store.sendChat(ptext);
            store.toast("Prompt sent to your agent");
          },
        };
      })
    : [];

  // export dropdown
  const onJExportToggle = () => store.setState({ jExportOpen: !s.jExportOpen });
  const jExports = [
    { label: "Markdown", ext: ".md", go: () => store.exportJournal("md") },
    { label: "Plain text", ext: ".txt", go: () => store.exportJournal("txt") },
    { label: "PDF (print)", ext: ".pdf", go: () => store.exportJournal("pdf") },
    { label: "Word", ext: ".doc", go: () => store.exportJournal("doc") },
  ];

  // capture & chat panel
  const jMicHint = s.jMicOn ? "listening…" : "voice";
  const jMicHintColor = s.jMicOn ? "var(--danger)" : "var(--tx-3)";
  const jMicBd = s.jMicOn ? "var(--danger)" : "var(--line-2)";
  const jMicBg = s.jMicOn ? "var(--danger-bg)" : "transparent";
  const jMicFg = s.jMicOn ? "var(--danger)" : "var(--tx-2)";
  const jMicTitle = s.jMicOn ? "Stop dictation" : "Start dictation";
  const jInterimShow = !!s.jInterim;

  const msgs = s.chatMsgs.map((m) => ({
    id: m.id,
    who: m.who,
    text: m.text,
    streaming: !!m.streaming,
    whoColor: m.who === "You" ? "var(--tx-3)" : "var(--accent-text)",
  }));
  const jMsgs = msgs.slice(-6);
  const jMsgsEmpty = msgs.length === 0;
  const chatThinking = s.chatStatus === "thinking" || s.chatStatus === "tool";
  const chatThinkingLabel = s.chatStatus === "tool" ? "running tools" : "reasoning";
  const chatBusy = s.chatStatus !== "ready";

  const onJChatSend = () => {
    const v = store.jChatInputEl ? store.jChatInputEl.value : "";
    if (store.jChatInputEl) store.jChatInputEl.value = "";
    store.sendChat(v);
  };

  return (
    <div className="journal-grid" style={{ padding: "20px 26px 24px", minHeight: "100%", boxSizing: "border-box" }}>
      {/* pages list */}
      <div style={{ display: "flex", flexDirection: "column", gap: 14, minHeight: 0 }}>
        <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
          <span style={{ fontFamily: "var(--font-display)", fontWeight: 420, fontSize: 24, flex: 1 }}>Journal</span>
          <button className="btn btn-ghost btn-sm" onClick={onJNew}>
            + Page
          </button>
        </div>
        <div style={{ display: "flex", flexDirection: "column", gap: 4 }}>
          <span className="eyebrow" style={{ padding: "0 2px 4px" }}>
            Daily notes
          </span>
          {jDaily.map((jp) => (
            <button
              key={jp.id}
              onClick={jp.go}
              style={{ fontFamily: "var(--font-sans)", display: "flex", alignItems: "center", gap: 8, textAlign: "left", fontSize: 13, fontWeight: jp.weight, padding: "7px 10px", border: "none", borderRadius: "var(--r-1)", cursor: "pointer", background: jp.bg, color: jp.color }}
            >
              <span className="mono tabular" style={{ fontSize: 12, flex: 1 }}>
                {jp.label}
              </span>
              {jp.pinned && <span style={{ width: 6, height: 6, borderRadius: 999, background: "var(--accent)" }}></span>}
            </button>
          ))}
        </div>
        <div style={{ display: "flex", flexDirection: "column", gap: 4 }}>
          <span className="eyebrow" style={{ padding: "0 2px 4px" }}>
            Pages
          </span>
          {jNamed.map((jp) => (
            <button
              key={jp.id}
              onClick={jp.go}
              style={{ fontFamily: "var(--font-sans)", display: "flex", alignItems: "center", gap: 8, textAlign: "left", fontSize: 13, fontWeight: jp.weight, padding: "7px 10px", border: "none", borderRadius: "var(--r-1)", cursor: "pointer", background: jp.bg, color: jp.color }}
            >
              <span style={{ flex: 1, whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>{jp.label}</span>
              {jp.pinned && <span style={{ width: 6, height: 6, borderRadius: 999, background: "var(--accent)" }}></span>}
            </button>
          ))}
        </div>
        <div style={{ flex: 1 }}></div>
        <p style={{ fontSize: 11, lineHeight: 1.55, color: "var(--tx-3)", margin: 0, borderTop: "1px solid var(--line-1)", paddingTop: 12 }}>
          Local and off-chain — encrypted at rest in your data dir. Agents and the harness write here only with your approval; pinned pages snapshot encrypted to the PIN daemon.
        </p>
      </div>

      {/* editor / viewer */}
      <div className="surface" style={{ display: "flex", flexDirection: "column", minHeight: 420 }}>
        <div style={{ display: "flex", alignItems: "center", flexWrap: "wrap", gap: "10px 12px", padding: "14px 18px", borderBottom: "1px solid var(--line-1)" }}>
          {jViewing && (
            <span style={{ fontFamily: "var(--font-display)", fontWeight: 440, fontSize: 18, flex: 1, minWidth: 140, whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>{jTitle}</span>
          )}
          {s.jEditing && <input ref={setJTitle} className="input" style={{ flex: 1, maxWidth: 340, height: 32, fontWeight: 500 }} />}
          <span className="mono" style={{ fontSize: 10, letterSpacing: ".08em", textTransform: "uppercase", color: "var(--tx-3)", whiteSpace: "nowrap" }}>
            {jMeta}
          </span>
          <span style={{ position: "relative", display: "inline-flex" }}>
            <button className="btn btn-ghost btn-sm" onClick={onJExportToggle}>
              Export
            </button>
            {s.jExportOpen && (
              <span
                className="cc-fade-up"
                style={{ position: "absolute", right: 0, top: 34, zIndex: 20, background: "var(--srf-2)", border: "1px solid var(--line-2)", borderRadius: "var(--r-2)", boxShadow: "var(--shadow-lift)", display: "flex", flexDirection: "column", minWidth: 170, overflow: "hidden" }}
              >
                {jExports.map((ex) => (
                  <button
                    key={ex.ext}
                    onClick={ex.go}
                    style={{ fontFamily: "var(--font-sans)", textAlign: "left", fontSize: 12.5, padding: "9px 14px", border: "none", borderBottom: "1px solid var(--line-1)", cursor: "pointer", background: "transparent", color: "var(--tx-1)", display: "flex", gap: 10, alignItems: "baseline" }}
                  >
                    <span style={{ flex: 1 }}>{ex.label}</span>
                    <span className="mono" style={{ fontSize: 9.5, color: "var(--tx-3)" }}>
                      {ex.ext}
                    </span>
                  </button>
                ))}
              </span>
            )}
          </span>
          <button className="btn btn-ghost btn-sm" onClick={onJPin} disabled={jPinDisabled}>
            {jPinLabel}
          </button>
          <button className="btn btn-secondary btn-sm" onClick={onJEdit}>
            {jEditLabel}
          </button>
        </div>

        {jViewing && (
          <div style={{ flex: 1, minHeight: 0, overflow: "auto", padding: "18px 20px", display: "flex", flexDirection: "column", gap: 2 }}>
            {jBlocks.map((b) => (
              <div key={b.key} style={{ display: "flex", gap: 10, alignItems: "baseline", padding: "4px 0 4px " + b.pad }}>
                <span style={{ width: 5, height: 5, borderRadius: 999, background: "var(--line-2)", flexShrink: 0, position: "relative", top: -3 }}></span>
                {b.agent && (
                  <span className="mono" style={{ fontSize: 9, letterSpacing: ".1em", textTransform: "uppercase", color: "#5d60c9", border: "1px solid #5d60c9", borderRadius: 999, padding: "1px 7px", flexShrink: 0, position: "relative", top: -1 }}>
                    agent
                  </span>
                )}
                {b.prompt && (
                  <span className="mono" style={{ fontSize: 9, letterSpacing: ".1em", textTransform: "uppercase", color: "var(--accent-text)", border: "1px solid var(--accent-text)", borderRadius: 999, padding: "1px 7px", flexShrink: 0, position: "relative", top: -1 }}>
                    prompt
                  </span>
                )}
                <span style={{ fontSize: 13.5, lineHeight: 1.6, color: "var(--tx-1)", flex: 1 }}>{b.text}</span>
                {b.prompt && (
                  <button className="btn btn-ghost btn-sm" onClick={b.run} style={{ flexShrink: 0 }}>
                    Run →
                  </button>
                )}
              </div>
            ))}
            {jBacklinksShow && (
              <div style={{ marginTop: 18, borderTop: "1px solid var(--line-1)", paddingTop: 14, display: "flex", flexDirection: "column", gap: 8 }}>
                <span className="eyebrow">Linked from</span>
                {jBacklinks.map((bl) => (
                  <button
                    key={bl.id}
                    onClick={bl.go}
                    style={{ fontFamily: "var(--font-sans)", textAlign: "left", background: "var(--srf-1)", border: "1px solid var(--line-1)", borderRadius: "var(--r-1)", padding: "9px 12px", cursor: "pointer", display: "flex", flexDirection: "column", gap: 2 }}
                  >
                    <span style={{ fontSize: 12.5, fontWeight: 500, color: "var(--tx-1)" }}>{bl.title}</span>
                    <span style={{ fontSize: 11.5, color: "var(--tx-3)", whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis", maxWidth: 520 }}>{bl.snippet}</span>
                  </button>
                ))}
              </div>
            )}
          </div>
        )}

        {s.jEditing && (
          <div style={{ flex: 1, minHeight: 0, display: "flex", flexDirection: "column" }}>
            <textarea
              ref={setJText}
              style={{ flex: 1, minHeight: 300, resize: "none", border: "none", outline: "none", background: "var(--srf-2)", color: "var(--tx-1)", fontFamily: "var(--font-mono)", fontSize: 12.5, lineHeight: 1.8, padding: "18px 20px" }}
            />
            <div className="mono" style={{ fontSize: 10, color: "var(--tx-3)", padding: "8px 18px", borderTop: "1px solid var(--line-1)" }}>
              one bullet per line · two leading spaces indent · [[Page name]] links · @agent marks agent entries · @prompt makes a runnable prompt
            </div>
          </div>
        )}
      </div>

      {/* capture & chat rail */}
      <div className="surface journal-capture" style={{ display: "flex", flexDirection: "column", minHeight: 420 }}>
        <div style={{ display: "flex", alignItems: "center", gap: 10, padding: "12px 16px", borderBottom: "1px solid var(--line-1)" }}>
          <span style={{ fontSize: 13.5, fontWeight: 500, flex: 1 }}>Capture &amp; chat</span>
          <span className="mono" style={{ fontSize: 9.5, letterSpacing: ".1em", textTransform: "uppercase", color: jMicHintColor }}>
            {jMicHint}
          </span>
        </div>
        <div ref={(el) => { store.jChatScrollEl = el; }} style={{ flex: 1, minHeight: 0, overflow: "auto", padding: "14px 16px", display: "flex", flexDirection: "column", gap: 12 }}>
          {jMsgsEmpty && (
            <p style={{ fontSize: 12, lineHeight: 1.6, color: "var(--tx-3)", margin: 0 }}>
              Talk or type. Save what lands as a note or a runnable prompt on this page, or send it straight to your agent — same agent, same approvals.
            </p>
          )}
          {jMsgs.map((m) => (
            <div key={m.id} style={{ display: "flex", flexDirection: "column", gap: 4 }}>
              <span className="mono" style={{ fontSize: 9, letterSpacing: ".12em", textTransform: "uppercase", color: m.whoColor }}>
                {m.who}
              </span>
              <span style={{ fontSize: 12.5, lineHeight: 1.55, color: "var(--tx-1)", whiteSpace: "pre-wrap" }}>
                {m.text}
                {m.streaming && <span style={{ display: "inline-block", width: 6, height: 12, background: "var(--accent)", marginLeft: 2, verticalAlign: -2, animation: "ccCaret 1s step-end infinite" }}></span>}
              </span>
            </div>
          ))}
          {chatThinking && (
            <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
              <span style={{ width: 26, height: 26, display: "inline-block", flexShrink: 0 }}>
                <LoaderMark size={26} />
              </span>
              <span className="mono" style={{ fontSize: 9.5, letterSpacing: ".1em", textTransform: "uppercase", color: "var(--tx-3)" }}>
                {chatThinkingLabel}
              </span>
            </div>
          )}
        </div>
        {jInterimShow && (
          <div style={{ padding: "8px 16px", borderTop: "1px dashed var(--line-2)" }}>
            <span style={{ fontSize: 12, lineHeight: 1.5, color: "var(--tx-2)", fontStyle: "italic" }}>{s.jInterim}</span>
          </div>
        )}
        <div style={{ display: "flex", flexDirection: "column", gap: 8, padding: "12px 16px", borderTop: "1px solid var(--line-1)" }}>
          <div style={{ display: "flex", gap: 8 }}>
            <button
              onClick={() => toggleMic(store)}
              title={jMicTitle}
              style={{ width: 36, height: 36, flexShrink: 0, borderRadius: 999, border: "1px solid " + jMicBd, background: jMicBg, color: jMicFg, cursor: "pointer", display: "inline-flex", alignItems: "center", justifyContent: "center" }}
            >
              <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
                <path d="M12 3 A3 3 0 0 1 15 6 V12 A3 3 0 0 1 9 12 V6 A3 3 0 0 1 12 3 Z"></path>
                <path d="M5 11 A7 7 0 0 0 19 11 M12 18 V21"></path>
              </svg>
            </button>
            <input
              ref={(el) => { store.jChatInputEl = el; }}
              className="input"
              placeholder="Speak or type a thought…"
              onKeyDown={(e) => {
                if (e.key === "Enter" && !e.shiftKey) {
                  e.preventDefault();
                  const v = (e.target as HTMLInputElement).value;
                  (e.target as HTMLInputElement).value = "";
                  store.sendChat(v);
                }
              }}
              style={{ flex: 1, height: 36 }}
            />
          </div>
          <div style={{ display: "flex", gap: 8 }}>
            <button className="btn btn-primary btn-sm" onClick={onJChatSend} disabled={chatBusy} style={{ flex: 1 }}>
              Send to agent
            </button>
            <button className="btn btn-ghost btn-sm" onClick={() => store.appendCapture("")}>
              Save note
            </button>
            <button className="btn btn-ghost btn-sm" onClick={() => store.appendCapture("@prompt ")}>
              Save prompt
            </button>
          </div>
          <span className="mono" style={{ fontSize: 9.5, color: "var(--tx-3)" }}>
            dictation via your browser's speech engine · captures stay in your journal until you send them
          </span>
        </div>
      </div>
    </div>
  );
}
