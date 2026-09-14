declare global {
  namespace App {
    // Reserved for SvelteKit application types.
  }

  interface Window {
    __TAURI_INTERNALS__?: unknown;
  }

  interface ImportMetaEnv {
    readonly VITE_PALBEACON_API_ORIGIN?: string;
  }

  interface ImportMeta {
    readonly env: ImportMetaEnv;
  }
}

export {};
