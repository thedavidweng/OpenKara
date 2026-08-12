import { Component, type ErrorInfo, type ReactNode } from "react";
import i18next from "@/lib/i18n";
import { BackendContext, type Backend } from "@/lib/backend";

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
  static contextType = BackendContext;
  declare context: Backend;

  constructor(props: ErrorBoundaryProps) {
    super(props);
    this.state = { hasError: false, error: null };
  }

  static getDerivedStateFromError(error: Error): ErrorBoundaryState {
    return { hasError: true, error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error("Uncaught error:", error, info.componentStack);
    void this.context.settings.windowReady();
  }

  componentDidMount() {
    i18next.on("languageChanged", this.handleLanguageChanged);
  }

  componentWillUnmount() {
    i18next.off("languageChanged", this.handleLanguageChanged);
  }

  private handleLanguageChanged = () => {
    if (this.state.hasError) {
      this.forceUpdate();
    }
  };

  private handleReload = () => {
    this.setState({ hasError: false, error: null });
    window.location.reload();
  };

  render() {
    if (this.state.hasError) {
      return (
        <div className="flex h-screen flex-col items-center justify-center gap-4 bg-[var(--color-bg)] p-8 text-center">
          <h1 className="text-xl font-semibold text-[var(--color-text)]">
            {i18next.t("errors.somethingWentWrong")}
          </h1>
          <p className="max-w-md text-[13px] text-[var(--color-text-dim)]">
            {this.state.error?.message ||
              i18next.t("errors.somethingWentWrong")}
          </p>
          <button
            type="button"
            onClick={this.handleReload}
            className="rounded-md bg-[var(--color-accent)] px-4 py-2 text-[13px] text-[var(--color-on-accent)] transition-colors hover:bg-[var(--color-accent-hover)]"
          >
            {i18next.t("common.tryAgain")}
          </button>
        </div>
      );
    }

    return this.props.children;
  }
}
