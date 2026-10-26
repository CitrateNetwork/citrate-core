/// <reference types="vite/client" />

// Build-stamp globals injected by vite.config.ts `define` (baked at build time). Used by Settings to
// show a distinguishable build id (the app version alone is identical across rebuilds).
declare const __BUILD_SHA__: string;
declare const __BUILD_TIME__: string;
declare const __APP_VERSION__: string;

interface ImportMetaEnv {
  /** FL round cards (0.5.2): "1" turns them on at build time; anything else is OFF. */
  readonly VITE_FL_ROUND_CARDS?: string;
}
