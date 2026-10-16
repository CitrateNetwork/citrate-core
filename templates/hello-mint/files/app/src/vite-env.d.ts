/// <reference types="vite/client" />

interface ImportMetaEnv {
  readonly VITE_TARGET?: string;
  readonly VITE_FORK_RPC_URL?: string;
  readonly VITE_CONTRACT_ADDRESS?: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}
