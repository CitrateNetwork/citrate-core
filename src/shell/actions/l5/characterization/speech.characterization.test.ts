// 0.6.0 L5-WP01 characterization L5-VOICE (voice out): reading replies aloud as it is on
// citrate-core a826e441 (lane L5 evidence E19). Read-aloud is off by default and speaks only
// through the system's own engine (speechSynthesis); with no engine nothing is spoken. L5-WP12
// (voice out: system voices kept, local TTS, provider TTS) replaces these with stricter tests, in a
// separate commit that says why. The protected suite src/agent/speech.test.ts stays green beside it.
import { describe, it, expect } from "vitest";
import { browserSpeech, speakReply } from "../../../../agent/speech";
import { freshState } from "../../../state";

describe("L5-WP01 characterization: read-aloud", () => {
  it("read_aloud_is_off_by_default", () => {
    expect(freshState("p1").hermesReadAloud).toBe(false);
  });

  it("speech_without_synthesis_is_unsupported_and_speaks_nothing", () => {
    // No speechSynthesis at all.
    const none = browserSpeech({} as unknown as typeof globalThis);
    expect(none.supported).toBe(false);
    expect(none.voices()).toEqual([]);
    expect(speakReply(none, "Hello there", null)).toEqual({ spoken: false, voice: null, reason: "this system has no speech engine" });
    // speechSynthesis without SpeechSynthesisUtterance is no engine either.
    const spoken: unknown[] = [];
    const half = browserSpeech({
      speechSynthesis: { getVoices: () => [], speak: (u: unknown) => spoken.push(u), cancel: () => {} },
    } as unknown as typeof globalThis);
    expect(half.supported).toBe(false);
    expect(speakReply(half, "Hello there", "Samantha").spoken).toBe(false);
    expect(spoken).toEqual([]);
  });
});
