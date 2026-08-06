/// <reference types="vite/client" />

interface ImportMetaEnv {
  readonly VITE_API_BASE: string;
  readonly VITE_FALLBACK_POLL_INTERVAL_MS: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}
