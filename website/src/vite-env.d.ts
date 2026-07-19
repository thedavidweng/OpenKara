/// <reference types="vite/client" />

import type { TauriMockResult } from "@/mock/tauri-mock-impl";

declare global {
  interface Window {
    __TAURI_INTERNALS__: TauriMockResult["internals"];
    __TAURI_EVENT_PLUGIN_INTERNALS__: TauriMockResult["eventPluginInternals"];
  }
}
