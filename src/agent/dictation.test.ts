// Hermes P4 / WP4.1 — dictation pure-logic tests. The DOM SpeechRecognition binding
// isn't exercised here (jsdom has no engine); the transcript-accumulation rule + the
// honest availability probe are.
import { describe, it, expect, afterEach } from "vitest";
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

// A minimal fake Web Speech engine to exercise the error/restart logic (jsdom has none).
class FakeSR {
  continuous = false;
  interimResults = false;
  lang = "";
  onresult: ((e: unknown) => void) | null = null;
  onerror: ((e: { error: string }) => void) | null = null;
  onend: (() => void) | null = null;
  starts = 0;
  start() {
    this.starts++;
  }
  stop() {
    /* the caller drives onend explicitly in tests */
  }
}

describe("dictation — error handling does not tight-loop (Adversarial F3)", () => {
  afterEach(() => {
    delete (window as unknown as { SpeechRecognition?: unknown }).SpeechRecognition;
  });

  it("a recurring non-permission error stops the session and reports, instead of auto-restarting", () => {
    const fake = new FakeSR();
    (window as unknown as { SpeechRecognition: unknown }).SpeechRecognition = function () {
      return fake;
    };
    const errors: string[] = [];
    const d = createDictation({ onFinal: () => {}, onError: (m) => errors.push(m) });
    expect(d.supported).toBe(true);
    d.start();
    expect(fake.starts).toBe(1);
    // A `network` error fires, then the engine idles (onend). The OLD code would
    // restart here (starts→2) and spin; the fix stops the session.
    fake.onerror?.({ error: "network" });
    fake.onend?.();
    expect(fake.starts).toBe(1, "no auto-restart after a non-permission error");
    expect(errors.some((m) => /network/i.test(m))).toBe(true);
  });

  it("permission denial reports the friendly message and does not restart", () => {
    const fake = new FakeSR();
    (window as unknown as { SpeechRecognition: unknown }).SpeechRecognition = function () {
      return fake;
    };
    const errors: string[] = [];
    const d = createDictation({ onFinal: () => {}, onError: (m) => errors.push(m) });
    d.start();
    fake.onerror?.({ error: "not-allowed" });
    fake.onend?.();
    expect(fake.starts).toBe(1);
    expect(errors.some((m) => /permission denied/i.test(m))).toBe(true);
  });
});
