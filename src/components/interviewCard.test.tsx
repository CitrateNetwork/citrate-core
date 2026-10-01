// HUP-S1.4 (US-1.2 AC1-3) — the interview card: pick/confirm a track, answer its questions (defaults
// prefilled), "Just use defaults" straight to the brief, then an editable brief whose save runs the
// sidecar's check and shows a refusal inline. Nothing here builds anything.
import { describe, it, expect, vi } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { InterviewCard, looksLikeBuildAsk, refusalReason, type InterviewApi } from "./InterviewCard";
import type { Brief, BriefDraft, InterviewTrack } from "../bridge/domains";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const CODE: InterviewTrack = {
  id: "code",
  title: "Code",
  summary: "Write or fix code",
  persona: "Builder",
  skills: ["repo-read", "test-run"],
  workflow: "code-change",
  workflow_available: false,
  ships_in: "0.5.0",
  gates: ["tests pass", "member reviews the diff"],
  questions: [
    { id: "lang", ask: "Which language?", choices: ["rust", "ts"], default: "rust" },
    { id: "scope", ask: "How big?", choices: [], default: "one crate" },
    { id: "tests", ask: "Add tests?", choices: ["yes", "no"], default: "yes" },
  ],
};
const CREATIVE: InterviewTrack = {
  ...CODE,
  id: "creative",
  title: "Creative",
  persona: "Maker",
  workflow: "creative-draft",
  gates: ["member approves the draft"],
  questions: [
    { id: "medium", ask: "What medium?", choices: ["poster", "post"], default: "poster" },
    { id: "tone", ask: "Tone?", choices: [], default: "warm" },
    { id: "size", ask: "Size?", choices: [], default: "A4" },
  ],
};

/** What the sidecar would build from (track, goal, answers): defaults for anything unanswered. */
function draftFor(t: InterviewTrack, goal: string, answers: Record<string, string>): BriefDraft {
  const brief: Brief = {
    track: t.id,
    goal,
    constraints: t.questions.map((q) => ({ id: q.id, ask: q.ask, answer: answers[q.id] ?? q.default, from_default: !(q.id in answers) })),
    persona: t.persona,
    skills: [...t.skills],
    workflow: t.workflow,
    workflow_available: t.workflow_available,
    ships_in: t.ships_in,
    gates: [...t.gates],
  };
  return { brief, markdown: "# Brief\n\n**Goal:** " + goal };
}

function api(over: Partial<InterviewApi> = {}): InterviewApi {
  return {
    tracks: vi.fn(async () => [CREATIVE, CODE]),
    briefCreate: vi.fn(async (track: string | null, goal: string, answers: Record<string, string>) =>
      draftFor(track === "creative" ? CREATIVE : CODE, goal, answers),
    ),
    briefCheck: vi.fn(async (b: Brief) => ({ ok: true, markdown: "# Brief\n\n**Goal:** " + b.goal })),
    ...over,
  };
}

async function mount(el: React.ReactElement): Promise<{ host: HTMLDivElement; root: Root }> {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(el);
  });
  await act(async () => {}); // let the tracks + suggestion load
  return { host, root };
}
const q = <T extends Element = HTMLElement>(host: HTMLElement, id: string) => host.querySelector(`[data-testid="${id}"]`) as T | null;
async function click(el: Element | null) {
  expect(el).toBeTruthy();
  await act(async () => {
    (el as HTMLElement).click();
  });
  await act(async () => {});
}
/** Set a React-controlled input/select/textarea value the way a user would. */
async function type(el: Element | null, value: string) {
  expect(el).toBeTruthy();
  const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
  const setter = Object.getOwnPropertyDescriptor(proto, "value")?.set;
  await act(async () => {
    setter?.call(el, value);
    el!.dispatchEvent(new Event(el instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
  });
}

describe("looksLikeBuildAsk — offer 'Plan it first' on build-style asks", () => {
  it("matches build/make/create asks", () => {
    for (const t of ["help me make an NFT project", "Build a landing page", "create a token contract", "can you write a sprint plan for me", "I want to design a poster"]) {
      expect(looksLikeBuildAsk(t), t).toBe(true);
    }
  });
  it("ignores questions about the node", () => {
    for (const t of ["What is my staking position?", "Network status", "", "how many peers do I have"]) {
      expect(looksLikeBuildAsk(t), t).toBe(false);
    }
  });
});

describe("refusalReason", () => {
  it("strips the BRIEF_REFUSED tag and an Error prefix", () => {
    expect(refusalReason(new Error("BRIEF_REFUSED: required gate removed: tests pass"))).toBe("required gate removed: tests pass");
    expect(refusalReason("Error: hermes is not running (no session bearer)")).toBe("hermes is not running (no session bearer)");
  });
});

describe("InterviewCard — interview", () => {
  it("preselects the sidecar's suggested track and prefills every default (AC1)", async () => {
    const a = api();
    const { host } = await mount(<InterviewCard goal="fix the parser bug" api={a} onAccept={() => {}} onCancel={() => {}} />);
    expect(a.briefCreate).toHaveBeenCalledWith(null, "fix the parser bug", {});
    expect(q<HTMLSelectElement>(host, "iv-track")!.value).toBe("code");
    expect(q<HTMLSelectElement>(host, "iv-q-lang")!.value).toBe("rust");
    expect(q<HTMLSelectElement>(host, "iv-q-lang")!.tagName).toBe("SELECT");
    expect(q<HTMLInputElement>(host, "iv-q-scope")!.value).toBe("one crate");
    expect(host.textContent).toContain("Which language?");
  });

  it("asks the member to pick when no track fits, then shows that track's questions", async () => {
    const a = api({ briefCreate: vi.fn(async () => { throw new Error("BRIEF_REFUSED: no track fits that goal; pick one from /tracks"); }) });
    const { host } = await mount(<InterviewCard goal="hmm" api={a} onAccept={() => {}} onCancel={() => {}} />);
    expect(q<HTMLSelectElement>(host, "iv-track")!.value).toBe("");
    expect(host.textContent).toContain("Pick a track");
    expect(q(host, "iv-defaults")!.hasAttribute("disabled")).toBe(true);
    await type(q(host, "iv-track"), "creative");
    expect(q<HTMLSelectElement>(host, "iv-q-medium")!.value).toBe("poster");
  });

  it("says honestly when the sidecar isn't running and offers a retry", async () => {
    const tracks = vi.fn(async (): Promise<InterviewTrack[]> => { throw new Error("hermes is not running (no session bearer)"); });
    const { host } = await mount(<InterviewCard goal="build a thing" api={api({ tracks })} onAccept={() => {}} onCancel={() => {}} />);
    expect(q(host, "iv-error")!.textContent).toContain("hermes is not running");
    expect(q(host, "iv-defaults")).toBeNull();
    tracks.mockImplementationOnce(async () => [CODE]);
    await click(q(host, "iv-retry"));
    expect(q(host, "iv-error")).toBeNull();
    expect(q(host, "iv-track")).toBeTruthy();
  });
});

describe("InterviewCard — defaults path (AC3)", () => {
  it("'Just use defaults' goes straight to the brief, and saving accepts it", async () => {
    const a = api();
    const onAccept = vi.fn();
    const { host } = await mount(<InterviewCard goal="fix the parser bug" api={a} onAccept={onAccept} onCancel={() => {}} />);
    await click(q(host, "iv-defaults"));
    expect(a.briefCreate).toHaveBeenLastCalledWith("code", "fix the parser bug", {});
    // AC2: goal, constraints, persona/skills/workflow and gates are all on the brief.
    expect(q<HTMLTextAreaElement>(host, "brief-goal")!.value).toBe("fix the parser bug");
    expect(q<HTMLInputElement>(host, "brief-persona")!.value).toBe("Builder");
    expect(q<HTMLInputElement>(host, "brief-skills")!.value).toBe("repo-read, test-run");
    const gates = q(host, "brief-gates")!;
    expect(gates.textContent).toContain("tests pass");
    expect(gates.textContent).toContain("member reviews the diff");
    const boxes = gates.querySelectorAll("input[type=checkbox]");
    expect(boxes.length).toBe(2);
    boxes.forEach((b) => expect((b as HTMLInputElement).disabled).toBe(true));
    expect(q(host, "brief-workflow")!.textContent).toMatch(/code-change.*not available yet \(ships in 0\.5\.0\)/);
    await click(q(host, "brief-save"));
    expect(a.briefCheck).toHaveBeenCalledTimes(1);
    expect(onAccept).toHaveBeenCalledTimes(1);
    const [brief, md] = onAccept.mock.calls[0];
    expect(brief.track).toBe("code");
    expect(brief.constraints.every((c: { from_default: boolean }) => c.from_default)).toBe(true);
    expect(md).toContain("fix the parser bug");
  });
});

describe("InterviewCard — edit path", () => {
  it("sends only changed answers, then saves the member's edits", async () => {
    const a = api();
    const onAccept = vi.fn();
    const { host } = await mount(<InterviewCard goal="fix the parser bug" api={a} onAccept={onAccept} onCancel={() => {}} />);
    await type(q(host, "iv-q-lang"), "ts");
    await click(q(host, "iv-write"));
    expect(a.briefCreate).toHaveBeenLastCalledWith("code", "fix the parser bug", { lang: "ts" });
    expect(q<HTMLSelectElement>(host, "brief-a-lang")!.value).toBe("ts");
    await type(q(host, "brief-goal"), "fix the parser bug and add a regression test");
    await type(q(host, "brief-persona"), "Careful Builder");
    await type(q(host, "brief-skills"), "repo-read, test-run, lint");
    await type(q(host, "brief-a-scope"), "two crates");
    await click(q(host, "brief-save"));
    const sent: Brief = (a.briefCheck as ReturnType<typeof vi.fn>).mock.calls[0][0];
    expect(sent.goal).toBe("fix the parser bug and add a regression test");
    expect(sent.persona).toBe("Careful Builder");
    expect(sent.skills).toEqual(["repo-read", "test-run", "lint"]);
    expect(sent.constraints.find((c) => c.id === "scope")).toMatchObject({ answer: "two crates", from_default: false });
    expect(sent.gates).toEqual(CODE.gates);
    expect(sent.workflow).toBe("code-change");
    expect(onAccept).toHaveBeenCalledTimes(1);
  });
});

describe("InterviewCard — refused edit", () => {
  it("shows the sidecar's reason inline and does not accept the brief", async () => {
    const a = api({ briefCheck: vi.fn(async () => { throw new Error("BRIEF_REFUSED: a brief names a persona"); }) });
    const onAccept = vi.fn();
    const { host } = await mount(<InterviewCard goal="fix the parser bug" api={a} onAccept={onAccept} onCancel={() => {}} />);
    await click(q(host, "iv-defaults"));
    await type(q(host, "brief-persona"), "");
    await click(q(host, "brief-save"));
    const err = q(host, "brief-error")!;
    expect(err.getAttribute("role")).toBe("alert");
    expect(err.textContent).toContain("a brief names a persona");
    expect(err.textContent).not.toContain("BRIEF_REFUSED");
    expect(onAccept).not.toHaveBeenCalled();
    // The brief stays editable so the member can fix it.
    expect(q(host, "brief-persona")).toBeTruthy();
  });
});
