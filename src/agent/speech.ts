// =====================================================================
// HUP-S3.7 — "TTS as an option": read Hermes's replies aloud in the persona's voice.
//
// Personas add no speech engine. A persona may name a voice id (`tts_voice`); this module looks it
// up among the voices the system's own speech engine has (the Web Speech API in the app's webview)
// and speaks with it, or with the system voice when the id is unset or not installed here. Reading
// aloud is off by default (store `hermesReadAloud`). Nothing leaves the device.
// =====================================================================

export interface SpeechVoice {
  name: string;
  voiceURI: string;
  lang: string;
  default: boolean;
}

/** The speech engine this module drives (the browser's in the app; a fake in tests). */
export interface SpeechEngine {
  supported: boolean;
  voices(): SpeechVoice[];
  speak(text: string, voice: SpeechVoice | null): void;
  cancel(): void;
}

/** Longest text handed to the engine in one reply. */
export const MAX_SPOKEN_CHARS = 4000;

/** The installed voice a persona's voice id names (by voice URI or name, case-insensitively). */
export function pickVoice(voices: readonly SpeechVoice[], id: string | null | undefined): SpeechVoice | null {
  const want = (id ?? "").trim().toLowerCase();
  if (!want) return null;
  return voices.find((v) => v.voiceURI.toLowerCase() === want) ?? voices.find((v) => v.name.toLowerCase() === want) ?? null;
}

/** Which voice will actually speak, for the settings screen. */
export function voiceLabel(voices: readonly SpeechVoice[], id: string | null | undefined): string {
  const want = (id ?? "").trim();
  if (!want) return "system voice";
  const v = pickVoice(voices, want);
  return v ? v.name : `system voice (${want} is not installed here)`;
}

/** The words of a markdown reply: code blocks dropped, marks removed, bounded. */
export function speakableText(markdown: string): string {
  const text = (markdown ?? "")
    .replace(/```[\s\S]*?```/g, " ")
    .replace(/`([^`]*)`/g, "$1")
    .replace(/!\[[^\]]*\]\([^)]*\)/g, " ")
    .replace(/\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/^\s*[-*+]\s+\[[ xX]\]\s*/gm, "")
    .replace(/^\s*[-*+]\s+/gm, "")
    .replace(/^\s*#{1,6}\s*/gm, "")
    .replace(/[*_~#>|]/g, "")
    .replace(/\s+/g, " ")
    .trim();
  return text.length > MAX_SPOKEN_CHARS ? text.slice(0, MAX_SPOKEN_CHARS) : text;
}

export type SpeakResult = { spoken: true; voice: string | null } | { spoken: false; voice: null; reason: string };

/** Speak a reply (stopping anything still being read). `voice` in the result is the persona voice
 *  that spoke, or null for the system voice. */
export function speakReply(engine: SpeechEngine, markdown: string, voiceId: string | null | undefined): SpeakResult {
  if (!engine.supported) return { spoken: false, voice: null, reason: "this system has no speech engine" };
  const text = speakableText(markdown);
  if (!text) return { spoken: false, voice: null, reason: "nothing to read" };
  const voice = pickVoice(engine.voices(), voiceId);
  engine.cancel();
  engine.speak(text, voice);
  return { spoken: true, voice: voice ? voice.name : null };
}

interface RawVoice {
  name: string;
  voiceURI: string;
  lang: string;
  default: boolean;
}
interface RawSynth {
  getVoices(): RawVoice[];
  speak(u: unknown): void;
  cancel(): void;
}

/** The browser's speech engine (`speechSynthesis`), or an unsupported one where there is none. */
export function browserSpeech(g: typeof globalThis = globalThis): SpeechEngine {
  const anyG = g as unknown as { speechSynthesis?: RawSynth; SpeechSynthesisUtterance?: new (text: string) => { voice: unknown } };
  const synth = anyG.speechSynthesis;
  const Utter = anyG.SpeechSynthesisUtterance;
  if (!synth || typeof Utter !== "function") {
    return { supported: false, voices: () => [], speak: () => undefined, cancel: () => undefined };
  }
  const raw = (): RawVoice[] => {
    try {
      return synth.getVoices() ?? [];
    } catch {
      return [];
    }
  };
  return {
    supported: true,
    voices: () => raw().map((v) => ({ name: v.name, voiceURI: v.voiceURI, lang: v.lang, default: !!v.default })),
    speak: (text, voice) => {
      const u = new Utter(text);
      if (voice) {
        const match = raw().find((v) => v.voiceURI === voice.voiceURI);
        if (match) u.voice = match;
      }
      synth.speak(u);
    },
    cancel: () => {
      try {
        synth.cancel();
      } catch {
        /* nothing being read */
      }
    },
  };
}
