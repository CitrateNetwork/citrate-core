// HUP-S3.7 — "TTS as an option": Hermes's replies read aloud with the persona's voice, through the
// system's own speech engine (no engine ships with personas). Off by default.
import { describe, it, expect, vi } from "vitest";
import { browserSpeech, pickVoice, speakableText, speakReply, voiceLabel, type SpeechEngine, type SpeechVoice } from "./speech";

const VOICES: SpeechVoice[] = [
  { name: "Samantha", voiceURI: "com.apple.voice.compact.en-US.Samantha", lang: "en-US", default: true },
  { name: "Daniel", voiceURI: "com.apple.voice.compact.en-GB.Daniel", lang: "en-GB", default: false },
];

function engine(voices = VOICES): SpeechEngine & { said: { text: string; voice: SpeechVoice | null }[] } {
  const said: { text: string; voice: SpeechVoice | null }[] = [];
  return {
    supported: true,
    voices: () => voices,
    speak: vi.fn((text: string, voice: SpeechVoice | null) => {
      said.push({ text, voice });
    }),
    cancel: vi.fn(),
    said,
  };
}

describe("speech voice choice", () => {
  it("matches a persona voice id by voice URI or name, case-insensitively", () => {
    expect(pickVoice(VOICES, "com.apple.voice.compact.en-GB.Daniel")?.name).toBe("Daniel");
    expect(pickVoice(VOICES, "daniel")?.name).toBe("Daniel");
  });

  it("no voice id, or one this system does not have, means the system voice (null)", () => {
    expect(pickVoice(VOICES, null)).toBeNull();
    expect(pickVoice(VOICES, undefined)).toBeNull();
    expect(pickVoice(VOICES, "Fiona")).toBeNull();
  });

  it("labels say which voice will actually speak", () => {
    expect(voiceLabel(VOICES, null)).toBe("system voice");
    expect(voiceLabel(VOICES, "Daniel")).toBe("Daniel");
    expect(voiceLabel(VOICES, "Fiona")).toBe("system voice (Fiona is not installed here)");
  });
});

describe("speakable text", () => {
  it("drops code blocks and markdown marks, keeps the words", () => {
    const t = speakableText("**Done:** shipped\n\n```sh\nforge test\n```\n- [ ] Next: *deploy* `cmd`\n# Heading");
    expect(t).not.toContain("forge test");
    expect(t).not.toMatch(/[*`#]/);
    expect(t).toContain("Done: shipped");
    expect(t).toContain("Next: deploy cmd");
    expect(t).toContain("Heading");
  });

  it("is bounded", () => {
    expect(speakableText("word ".repeat(5000)).length).toBeLessThanOrEqual(4000);
  });
});

describe("speakReply", () => {
  it("speaks the reply with the persona's voice when it is installed", () => {
    const e = engine();
    const r = speakReply(e, "Hello there", "Daniel");
    expect(r).toEqual({ spoken: true, voice: "Daniel" });
    expect(e.cancel).toHaveBeenCalled();
    expect(e.said).toEqual([{ text: "Hello there", voice: VOICES[1] }]);
  });

  it("falls back to the system voice and says so", () => {
    const e = engine();
    const r = speakReply(e, "Hello", "Fiona");
    expect(r).toEqual({ spoken: true, voice: null });
    expect(e.said[0].voice).toBeNull();
  });

  it("an unsupported engine or nothing to say speaks nothing, honestly", () => {
    const off: SpeechEngine = { supported: false, voices: () => [], speak: vi.fn(), cancel: vi.fn() };
    expect(speakReply(off, "Hello", null)).toEqual({ spoken: false, voice: null, reason: "this system has no speech engine" });
    expect(off.speak).not.toHaveBeenCalled();
    const e = engine();
    expect(speakReply(e, "```\ncode only\n```", null).spoken).toBe(false);
    expect(e.speak).not.toHaveBeenCalled();
  });

  it("browserSpeech reports unsupported where there is no speechSynthesis", () => {
    const e = browserSpeech({} as unknown as typeof globalThis);
    expect(e.supported).toBe(false);
    expect(e.voices()).toEqual([]);
    expect(() => e.speak("x", null)).not.toThrow();
  });

  it("browserSpeech speaks through speechSynthesis with the chosen voice", () => {
    const spoken: { text: string; voice: unknown }[] = [];
    class Utter {
      text: string;
      voice: unknown = null;
      constructor(t: string) {
        this.text = t;
      }
    }
    const raw = { name: "Daniel", voiceURI: "u-daniel", lang: "en-GB", default: false };
    const g = {
      speechSynthesis: {
        getVoices: () => [raw],
        speak: (u: Utter) => spoken.push({ text: u.text, voice: u.voice }),
        cancel: vi.fn(),
      },
      SpeechSynthesisUtterance: Utter,
    } as unknown as typeof globalThis;
    const e = browserSpeech(g);
    expect(e.supported).toBe(true);
    expect(e.voices().map((v) => v.name)).toEqual(["Daniel"]);
    e.speak("hi", e.voices()[0]);
    expect(spoken).toEqual([{ text: "hi", voice: raw }]);
  });
});
