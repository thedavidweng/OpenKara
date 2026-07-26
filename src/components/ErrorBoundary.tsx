import { Component, type ErrorInfo, type ReactNode } from "react";
import { windowReady } from "@/lib/tauri";

interface ErrorBoundaryProps {
  children: ReactNode;
}

interface ErrorBoundaryState {
  hasError: boolean;
  error: Error | null;
}

export class ErrorBoundary extends Component<
  ErrorBoundaryProps,
  ErrorBoundaryState
> {
  constructor(props: ErrorBoundaryProps) {
    super(props);
    this.state = { hasError: false, error: null };
  }

  static getDerivedStateFromError(error: Error): ErrorBoundaryState {
    return { hasError: true, error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error("Uncaught error:", error, info.componentStack);
    // The main window starts hidden and is revealed by the `window_ready`
    // handshake, which a crash on the way to the first screen never reaches.
    // The native watchdog covers this too, but it only knows the window is
    // stuck after its deadline — the boundary knows now, so the panel below
    // becomes visible immediately instead of after a blank pause.
    void windowReady();
  }

  private handleReload = () => {
    this.setState({ hasError: false, error: null });
    window.location.reload();
  };

  render() {
    if (this.state.hasError) {
      return (
        <div className="flex h-screen flex-col items-center justify-center gap-4 bg-[var(--color-bg)] p-8 text-center">
          <h1 className="text-xl font-semibold text-[var(--color-text)]">
            Something went wrong
          </h1>
          <p className="max-w-md text-[13px] text-[var(--color-text-dim)]">
            {this.state.error?.message || "An unexpected error occurred."}
          </p>
          <button
            onClick={this.handleReload}
            className="rounded-md bg-[var(--color-accent)] px-4 py-2 text-[13px] text-[var(--color-on-accent)] transition-colors hover:bg-[var(--color-accent-hover)]"
          >
            Reload
          </button>
        </div>
      );
    }

    return this.props.children;
  }
}
