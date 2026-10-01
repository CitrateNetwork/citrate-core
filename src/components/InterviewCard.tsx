// =====================================================================
// InterviewCard — HUP-S1.4 (US-1.2): "interview before building".
//
// Hermes asks a short, track-specific set of questions (3–7, every one with a default), then shows
// a brief the member edits before anything is built. The tracks, the brief and every rule about it
// come from the Hermes sidecar (runtime `agent-loop::interview`, served at /tracks, /briefs and
// /briefs/check through core's bearer-authed control client). This card only collects answers and
// edits; the sidecar decides what a valid brief is (required gates and the workflow can't be edited
// away) and its refusal reason is shown inline. Accepting a brief starts NOTHING: workflows whose
// status is "not available yet" say so (Rule 1).
// =====================================================================
import { useEffect, useState } from "react";
import type { Brief, BriefDraft, InterviewTrack } from "../bridge/domains";

/** The three sidecar calls the card needs (bridge.agentHarness implements them). */
export interface InterviewApi {
  tracks(): Promise<InterviewTrack[]>;
  briefCreate(track: string | null, goal: string, answers: Record<string, string>): Promise<BriefDraft>;
  briefCheck(brief: Brief): Promise<{ ok: boolean; markdown: string }>;
}

const BUILD_WORDS = /\b(build|make|create|write|design|draft|ship|launch|deploy|mint|plan|code|refactor)\b/i;

/** A build-style ask ("help me make an NFT project") — the chat offers "Plan it first" for these. */
export function looksLikeBuildAsk(text: string): boolean {
  return BUILD_WORDS.test(text || "");
}

/** The human part of a rejected call: drop the `Error:` / `BRIEF_REFUSED:` tags. */
export function refusalReason(e: unknown): string {
  const raw = e instanceof Error ? e.message : String(e ?? "");
  return raw.replace(/^Error:\s*/, "").replace(/^BRIEF_REFUSED:\s*/, "").trim() || "the request failed";
}

function defaultsOf(t: InterviewTrack | undefined): Record<string, string> {
  const out: Record<string, string> = {};
  for (const q of t?.questions ?? []) out[q.id] = q.default;
  return out;
}

/** Only the answers the member changed; everything else takes its default sidecar-side (so the brief
 *  marks it "(default)"). */
function changedAnswers(t: InterviewTrack, answers: Record<string, string>): Record<string, string> {
  const out: Record<string, string> = {};
  for (const q of t.questions) {
    const a = (answers[q.id] ?? "").trim();
    if (a !== "" && a !== q.default) out[q.id] = a;
  }
  return out;
}

export function workflowStatus(b: Pick<Brief, "workflow_available" | "ships_in">): string {
  if (b.workflow_available) return "available";
  return "not available yet" + (b.ships_in ? ` (ships in ${b.ships_in})` : "") + ". Nothing is built from this brief until the workflow ships.";
}

const lbl = { fontFamily: "var(--font-mono)", fontSize: 10, letterSpacing: ".12em", textTransform: "uppercase" as const, color: "var(--tx-3)" };
const row = { display: "flex", flexDirection: "column" as const, gap: 4 };

function AnswerField({ q, value, onChange, testId }: { q: InterviewTrack["questions"][number]; value: string; onChange: (v: string) => void; testId: string }) {
  return (
    <label style={row}>
      <span style={{ fontSize: 12.5, color: "var(--tx-2)" }}>{q.ask}</span>
      {q.choices.length > 0 ? (
        <select className="input" data-testid={testId} value={value} onChange={(e) => onChange(e.target.value)}>
          {q.choices.map((c) => (
            <option key={c} value={c}>
              {c}
              {c === q.default ? " (default)" : ""}
            </option>
          ))}
        </select>
      ) : (
        <input className="input" data-testid={testId} value={value} placeholder={q.default} onChange={(e) => onChange(e.target.value)} />
      )}
    </label>
  );
}

export function InterviewCard({ goal: initialGoal, api, onAccept, onCancel }: { goal: string; api: InterviewApi; onAccept: (brief: Brief, markdown: string) => void; onCancel: () => void }) {
  const [phase, setPhase] = useState<"loading" | "error" | "interview" | "brief">("loading");
  const [loadErr, setLoadErr] = useState("");
  const [tracks, setTracks] = useState<InterviewTrack[]>([]);
  const [trackId, setTrackId] = useState("");
  const [goal, setGoal] = useState(initialGoal.trim());
  const [answers, setAnswers] = useState<Record<string, string>>({});
  const [ivErr, setIvErr] = useState("");
  const [brief, setBrief] = useState<Brief | null>(null);
  const [skillsText, setSkillsText] = useState("");
  const [briefErr, setBriefErr] = useState("");
  const [busy, setBusy] = useState(false);
  const [attempt, setAttempt] = useState(0);

  const track = tracks.find((t) => t.id === trackId);

  useEffect(() => {
    let live = true;
    (async () => {
      setPhase("loading");
      setLoadErr("");
      let list: InterviewTrack[];
      try {
        list = await api.tracks();
      } catch (e) {
        if (live) {
          setLoadErr(refusalReason(e));
          setPhase("error");
        }
        return;
      }
      // The sidecar's first guess at the track (the member can always pick another). A refusal
      // means nothing fits: leave the pick empty and ask.
      let suggested = "";
      if (initialGoal.trim()) {
        try {
          suggested = (await api.briefCreate(null, initialGoal.trim(), {})).brief.track;
        } catch {
          suggested = "";
        }
      }
      if (!live) return;
      setTracks(list);
      const t = list.find((x) => x.id === suggested);
      setTrackId(t ? t.id : "");
      setAnswers(defaultsOf(t));
      setPhase("interview");
    })();
    return () => {
      live = false;
    };
  }, [api, initialGoal, attempt]);

  const pickTrack = (id: string) => {
    setTrackId(id);
    setAnswers(defaultsOf(tracks.find((t) => t.id === id)));
    setIvErr("");
  };

  const toBrief = async (useDefaults: boolean) => {
    if (!track) return;
    setBusy(true);
    setIvErr("");
    try {
      const d = await api.briefCreate(track.id, goal.trim(), useDefaults ? {} : changedAnswers(track, answers));
      setBrief(d.brief);
      setSkillsText(d.brief.skills.join(", "));
      setBriefErr("");
      setPhase("brief");
    } catch (e) {
      setIvErr(refusalReason(e));
    } finally {
      setBusy(false);
    }
  };

  const save = async () => {
    if (!brief) return;
    const edited: Brief = {
      ...brief,
      skills: skillsText
        .split(",")
        .map((s) => s.trim())
        .filter(Boolean),
    };
    setBusy(true);
    setBriefErr("");
    try {
      const r = await api.briefCheck(edited);
      onAccept(edited, r.markdown);
    } catch (e) {
      setBriefErr(refusalReason(e));
    } finally {
      setBusy(false);
    }
  };

  const setAnswer = (id: string, v: string) => {
    if (!brief) return;
    setBrief({ ...brief, constraints: brief.constraints.map((c) => (c.id === id ? { ...c, answer: v, from_default: false } : c)) });
  };

  return (
    <div data-testid="interview-card" style={{ border: "1px solid var(--line-2)", borderRadius: "var(--r-2, 8px)", background: "var(--srf-1)", padding: 14, display: "flex", flexDirection: "column", gap: 12, maxHeight: 420, overflow: "auto" }}>
      <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
        <span style={{ fontSize: 13.5, fontWeight: 500 }}>{phase === "brief" ? "Your brief" : "Plan it first"}</span>
        <span style={{ flex: 1 }} />
        <button className="btn btn-sm btn-ghost" onClick={onCancel}>
          Close
        </button>
      </div>

      {phase === "loading" && <span style={{ fontSize: 12.5, color: "var(--tx-3)" }}>Loading the interview…</span>}

      {phase === "error" && (
        <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
          <span role="alert" data-testid="iv-error" style={{ flex: 1, fontSize: 12.5, color: "var(--danger)" }}>
            The interview isn't available: {loadErr}. Hermes has to be running (Agent → Start).
          </span>
          <button className="btn btn-sm" data-testid="iv-retry" onClick={() => setAttempt((n) => n + 1)}>
            Retry
          </button>
        </div>
      )}

      {phase === "interview" && (
        <>
          <label style={row}>
            <span style={lbl}>Goal</span>
            <input className="input" data-testid="iv-goal" value={goal} onChange={(e) => setGoal(e.target.value)} placeholder="What do you want to make?" />
          </label>
          <label style={row}>
            <span style={lbl}>Track</span>
            <select className="input" data-testid="iv-track" value={trackId} onChange={(e) => pickTrack(e.target.value)}>
              <option value="">Pick a track…</option>
              {tracks.map((t) => (
                <option key={t.id} value={t.id}>
                  {t.title}
                </option>
              ))}
            </select>
            {track ? (
              <span style={{ fontSize: 12, color: "var(--tx-3)" }}>{track.summary}</span>
            ) : (
              <span style={{ fontSize: 12, color: "var(--tx-3)" }}>Pick a track: none was a clear fit for that goal.</span>
            )}
          </label>
          {track?.questions.map((q) => (
            <AnswerField key={q.id} q={q} testId={`iv-q-${q.id}`} value={answers[q.id] ?? q.default} onChange={(v) => setAnswers((a) => ({ ...a, [q.id]: v }))} />
          ))}
          {ivErr && (
            <span role="alert" data-testid="iv-refused" style={{ fontSize: 12.5, color: "var(--danger)" }}>
              {ivErr}
            </span>
          )}
          <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
            <button className="btn btn-sm btn-ghost" data-testid="iv-defaults" disabled={!track || !goal.trim() || busy} onClick={() => void toBrief(true)}>
              Just use defaults
            </button>
            <button className="btn btn-sm btn-primary" data-testid="iv-write" disabled={!track || !goal.trim() || busy} onClick={() => void toBrief(false)}>
              Write the brief
            </button>
          </div>
        </>
      )}

      {phase === "brief" && brief && (
        <>
          <label style={row}>
            <span style={lbl}>Goal</span>
            <textarea className="input" data-testid="brief-goal" value={brief.goal} onChange={(e) => setBrief({ ...brief, goal: e.target.value })} style={{ minHeight: 56, height: "auto", resize: "vertical" }} />
          </label>
          <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
            <span style={lbl}>Constraints</span>
            {brief.constraints.map((c) => {
              const q = track?.questions.find((x) => x.id === c.id) ?? { id: c.id, ask: c.ask, choices: [], default: c.answer };
              return <AnswerField key={c.id} q={q} testId={`brief-a-${c.id}`} value={c.answer} onChange={(v) => setAnswer(c.id, v)} />;
            })}
          </div>
          <label style={row}>
            <span style={lbl}>Persona</span>
            <input className="input" data-testid="brief-persona" value={brief.persona} onChange={(e) => setBrief({ ...brief, persona: e.target.value })} />
          </label>
          <label style={row}>
            <span style={lbl}>Skills (comma-separated)</span>
            <input className="input" data-testid="brief-skills" value={skillsText} onChange={(e) => setSkillsText(e.target.value)} />
          </label>
          <div style={row}>
            <span style={lbl}>Workflow</span>
            <span data-testid="brief-workflow" style={{ fontSize: 12.5, color: brief.workflow_available ? "var(--tx-1)" : "var(--tx-2)" }}>
              <code>{brief.workflow}</code>: {workflowStatus(brief)}
            </span>
          </div>
          <div style={row} data-testid="brief-gates">
            <span style={lbl}>Gates that will apply (set by the track)</span>
            {brief.gates.map((g) => (
              <label key={g} style={{ display: "flex", alignItems: "center", gap: 8, fontSize: 12.5, color: "var(--tx-2)" }}>
                <input type="checkbox" checked={false} disabled readOnly />
                {g}
              </label>
            ))}
          </div>
          {briefErr && (
            <span role="alert" data-testid="brief-error" style={{ fontSize: 12.5, color: "var(--danger)" }}>
              Hermes didn't accept this brief: {briefErr}
            </span>
          )}
          <div style={{ display: "flex", gap: 8, alignItems: "center", flexWrap: "wrap" }}>
            <button className="btn btn-sm btn-ghost" onClick={() => setPhase("interview")} disabled={busy}>
              Back
            </button>
            <button className="btn btn-sm btn-primary" data-testid="brief-save" onClick={() => void save()} disabled={busy}>
              Save brief
            </button>
            <span style={{ fontSize: 11.5, color: "var(--tx-3)" }}>Saving keeps the brief in this chat. Nothing is built.</span>
          </div>
        </>
      )}
    </div>
  );
}
