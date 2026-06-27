import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./styles.css";

class ErrorBoundary extends React.Component<
  { children: React.ReactNode },
  { error: Error | null }
> {
  state = { error: null as Error | null };

  static getDerivedStateFromError(error: Error) {
    return { error };
  }

  render() {
    if (this.state.error) {
      return (
        <div className="app-error-fallback">
          <h1>WaveTrace failed to load</h1>
          <pre>{this.state.error.message}</pre>
          <div className="app-error-actions">
            <button type="button" className="primary" onClick={() => window.location.reload()}>
              Reload
            </button>
          </div>
          <p className="muted">
            If this keeps happening after long runs, restart the app from the tray menu or Exit
            button. Dev builds also need <code>npm run tauri dev</code> running.
          </p>
        </div>
      );
    }
    return this.props.children;
  }
}

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <ErrorBoundary>
      <App />
    </ErrorBoundary>
  </React.StrictMode>
);
