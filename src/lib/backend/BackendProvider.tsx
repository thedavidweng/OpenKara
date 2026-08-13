import type { ReactNode } from "react";
import { BackendContext } from "./context";
import type { Backend } from "./types";

export function BackendProvider({
  backend,
  children,
}: {
  backend: Backend;
  children: ReactNode;
}) {
  return (
    <BackendContext.Provider value={backend}>
      {children}
    </BackendContext.Provider>
  );
}
