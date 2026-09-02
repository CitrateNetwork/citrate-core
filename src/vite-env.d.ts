/// <reference types="vite/client" />

// Build-stamp globals injected by vite.config.ts `define` (baked at build time). Used by Settings to
// show a distinguishable build id (the app version alone is identical across rebuilds).
declare const __BUILD_SHA__: string;
declare const __BUILD_TIME__: string;
declare const __APP_VERSION__: string;
