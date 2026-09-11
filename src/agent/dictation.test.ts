// Hermes P4 / WP4.1 — dictation pure-logic tests. The DOM SpeechRecognition binding
// isn't exercised here (jsdom has no engine); the transcript-accumulation rule + the
// honest availability probe are.
import { describe, it, expect } from "vitest";
import { appendFinal, dictationSupported, createDictation } from "./dictation";

describe("dictation — appendFinal", () => {
  it("appends a fragment with a single separating space", () => {
    expect(appendFinal("hello", "world")).toBe("hello world");
  });
  it("seeds an empty input without a leading space", () => {
    expect(appendFinal("", "first words")).toBe("first words");
    expect(appendFinal("   ", "first")).toBe("first");
  });
  it("ignores an empty/whitespace fragment (no trailing space added)", () => {
    expect(appendFinal("keep", "   ")).toBe("keep");
    expect(appendFinal("keep", "")).toBe("keep");
  });
  it("trims the fragment it appends", () => {
    expect(appendFinal("a", "  b  ")).toBe("a b");
  });
});

describe("dictation — availability is honest (Rule 1)", () => {
  it("reports unsupported where the Web Speech API is absent (jsdom)", () => {
    expect(dictationSupported()).toBe(false);
  });
  it("createDictation is a safe no-op controller when unsupported", () => {
    const d = createDictation({ onFinal: () => {} });
    expect(d.supported).toBe(false);
    // start/stop must not throw even with no engine.
    expect(() => {
      d.start();
      d.stop();
    }).not.toThrow();
  });
});
