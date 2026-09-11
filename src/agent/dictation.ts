// =====================================================================
// Hermes P4 / WP4.1 — voice-to-text (dictation).
//
// On-device by default: the browser's Web Speech API (Chrome/Edge) transcribes
// locally with no backend. This generalizes the journal's proven dictation loop so
// ANY chat input can use it. Honest fallback (Rule 1): where Web Speech is absent,
// `supported` is false and the caller shows "typing works everywhere" rather than a
// fake mic.
//
// Higher-quality upgrade (follow-on): the on-device whisper sidecar / gateway
// `POST /v1/audio/transcriptions` (OpenAI-compatible, confirmed by the DGX team). That
// path records audio and posts it; it slots in behind the same caller UI when the
// sidecar is bundled / the gateway STT endpoint merges. Until then, Web Speech is the
// real, working on-device path.
// =====================================================================

/** Append a newly-finalized fragment to the current input text without doubling
 *  spaces or leading whitespace. Pure — the transcript-accumulation rule the DOM
 *  binding relies on, unit-tested without a browser. */
export function appendFinal(current: string, fragment: string): string {
  const frag = fragment.trim();
  if (!frag) return current;
  const base = current.trim();
  return base ? `${base} ${frag}` : frag;
}

// Minimal shapes for the Web Speech API (not in lib.dom types).
interface SpeechLike {
  continuous: boolean;
  interimResults: boolean;
  lang: string;
  onresult: ((e: SpeechResultEvent) => void) | null;
  onerror: ((e: { error: string }) => void) | null;
  onend: (() => void) | null;
  start(): void;
  stop(): void;
}
interface SpeechResultEvent {
  resultIndex: number;
  results: { length: number; [i: number]: { isFinal: boolean; 0: { transcript: string } } };
}
type SpeechWindow = typeof window & {
  SpeechRecognition?: new () => SpeechLike;
  webkitSpeechRecognition?: new () => SpeechLike;
};

/** Is on-device browser dictation available here? (Chrome/Edge expose it.) */
export function dictationSupported(): boolean {
  if (typeof window === "undefined") return false;
  const w = window as SpeechWindow;
  return !!(w.SpeechRecognition || w.webkitSpeechRecognition);
}

export interface DictationCallbacks {
  /** A finalized fragment (append it to the input). */
  onFinal: (fragment: string) => void;
  /** The live interim guess (show it faintly; not yet committed). */
  onInterim?: (text: string) => void;
  /** A plain-language error (e.g. permission denied). */
  onError?: (message: string) => void;
  /** Fired when recognition actually stops. */
  onEnd?: () => void;
}

export interface Dictation {
  readonly supported: boolean;
  /** Begin listening. Auto-restarts across the engine's idle `onend` until `stop()`. */
  start(): void;
  /** Stop listening. */
  stop(): void;
}

/** Create a dictation controller over the Web Speech API. `supported` is false (and
 *  start/stop are no-ops) where the API is absent — the caller shows an honest
 *  fallback. Mirrors the journal loop: continuous + interim, auto-restart until stop. */
export function createDictation(cb: DictationCallbacks): Dictation {
  if (!dictationSupported()) {
    return { supported: false, start() {}, stop() {} };
  }
  const w = window as SpeechWindow;
  const SR = (w.SpeechRecognition || w.webkitSpeechRecognition)!;
  const r = new SR();
  r.continuous = true;
  r.interimResults = true;
  r.lang = "en-US";
  let on = false;
  r.onresult = (e) => {
    let interim = "";
    let fin = "";
    for (let i = e.resultIndex; i < e.results.length; i++) {
      const t = e.results[i][0].transcript;
      if (e.results[i].isFinal) fin += t;
      else interim += t;
    }
    if (fin) cb.onFinal(fin);
    cb.onInterim?.(interim);
  };
  r.onerror = (e) => {
    // A permission denial is terminal. Any OTHER error (network, service failure) must
    // NOT silently auto-restart — that spins a tight error→onend→start loop. Stop the
    // session and report honestly; the member can re-toggle the mic. (Adversarial F3.)
    on = false;
    if (e.error === "not-allowed" || e.error === "service-not-allowed") {
      cb.onError?.("Microphone permission denied — typing still works");
    } else if (e.error && e.error !== "no-speech" && e.error !== "aborted") {
      cb.onError?.(`Dictation stopped (${e.error}) — typing still works`);
    }
  };
  r.onend = () => {
    if (on) {
      // The engine idles out periodically; restart to keep a continuous session.
      try {
        r.start();
      } catch {
        /* ignore a double-start race */
      }
    } else {
      cb.onEnd?.();
    }
  };
  return {
    supported: true,
    start() {
      on = true;
      try {
        r.start();
      } catch {
        /* already started */
      }
    },
    stop() {
      on = false;
      try {
        r.stop();
      } catch {
        /* already stopped */
      }
    },
  };
}
