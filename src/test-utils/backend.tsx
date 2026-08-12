import {
  render,
  type RenderOptions,
  type RenderResult,
} from "@testing-library/react";
import type { ReactElement, ReactNode } from "react";
import { BackendProvider, type Backend } from "@/lib/backend";

export function withBackend(backend: Backend) {
  return function BackendWrapper({ children }: { children: ReactNode }) {
    return <BackendProvider backend={backend}>{children}</BackendProvider>;
  };
}

export function renderWithBackend(
  ui: ReactElement,
  backend: Backend,
  options?: Omit<RenderOptions, "wrapper">,
): RenderResult {
  return render(ui, { ...options, wrapper: withBackend(backend) });
}
