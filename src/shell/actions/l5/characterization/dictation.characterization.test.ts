// 0.6.0 L5-WP01 characterization L5-VOICE (voice in): dictation as it is on citrate-core a826e441
// (lane L5 evidence E19). It uses only the webview's Web Speech recognition; where that is absent
// it is unsupported and start does nothing (no microphone, no recording, no other engine). L5-WP11
// (push-to-talk with local and provider STT) replaces this with a stricter test, in a separate
// commit that says why. The protected suite src/agent/dictation.test.ts stays green beside it.
import { describe, it, expect } from "vitest";
import { createDictation, dictationSupported } from "../../../../agent/dictation";

describe("L5-WP01 characterization: dictation", () => {
  it("dictation_is_unsupported_without_web_speech", () => {
    const w = window as unknown as Record<string, unknown>;
    expect(w.SpeechRecognition).toBeUndefined();
    expect(w.webkitSpeechRecognition).toBeUndefined();
    expect(dictationSupported()).toBe(false);
    const heard: string[] = [];
    const errors: string[] = [];
    const d = createDictation({ onFinal: (t) => heard.push(t), onError: (e) => errors.push(e) });
    expect(d.supported).toBe(false);
    expect(() => {
      d.start();
      d.stop();
    }).not.toThrow();
    expect(heard).toEqual([]);
    expect(errors).toEqual([]);
  });
});
