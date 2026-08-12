import { createContext, useContext } from "react";
import { tauriBackend } from "./tauri-backend";
import type { Backend } from "./types";

export const BackendContext = createContext<Backend>(tauriBackend);

export function useBackend(): Backend {
  return useContext(BackendContext);
}
