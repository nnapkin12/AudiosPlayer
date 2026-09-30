import { Component, type ErrorInfo, type ReactNode } from "react";

interface Props {
  /** Shown in the fallback, e.g. "Library" or "the app". */
  name: string;
  children: ReactNode;
  /** When true the fallback fills its parent instead of a small card. */
  fill?: boolean;
}

interface State {
  error: Error | null;
}

/**
 * Keeps a render error inside one view. Without this a single throw blanks
 * the whole frameless window, including the close button.
 */
export class ErrorBoundary extends Component<Props, State> {
  state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error(`Audios! ${this.props.name} crashed`, error, info.componentStack);
  }

  render() {
    const { error } = this.state;
    if (!error) return this.props.children;
    return (
      <div
        role="alert"
        className={`flex ${this.props.fill ? "min-h-0 flex-1" : ""} flex-col items-center justify-center gap-3 p-8 text-center`}
      >
        <p className="text-[16px] font-semibold text-app-text">
          Something went wrong in {this.props.name}.
        </p>
        <p className="max-w-lg break-words text-[13px] text-app-muted">
          {error.message || String(error)}
        </p>
        <div className="flex gap-2">
          <button
            type="button"
            onClick={() => this.setState({ error: null })}
            className="rounded-md border border-app-border px-3 py-1.5 text-[13px] font-semibold hover:bg-app-hover"
          >
            Try again
          </button>
          <button
            type="button"
            onClick={() => window.location.reload()}
            className="rounded-md bg-app-play px-3 py-1.5 text-[13px] font-semibold text-app-play-fg"
          >
            Reload Audios!
          </button>
        </div>
      </div>
    );
  }
}
