import { useCallback, useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import type { Update } from "@tauri-apps/plugin-updater";
import { logUiError } from "../uiError";
import {
  fetchUpdate,
  getUpdateChannel,
  installUpdate,
  isUpdaterEnabled,
  type UpdateProgress,
} from "../updater";

function formatBytes(value: number): string {
  if (value <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB"];
  const power = Math.min(
    Math.floor(Math.log(value) / Math.log(1024)),
    units.length - 1
  );
  const scaled = value / 1024 ** power;
  return `${scaled.toFixed(power === 0 ? 0 : 1)} ${units[power]}`;
}

interface AppUpdaterProps {
  /** Check on mount (startup). */
  autoCheck?: boolean;
  /** Compact row for Settings; banner mode for header. */
  variant?: "settings" | "banner";
}

export default function AppUpdater({
  autoCheck = false,
  variant = "settings",
}: AppUpdaterProps) {
  const [appVersion, setAppVersion] = useState<string | null>(null);
  const [pending, setPending] = useState<Update | null>(null);
  const [progress, setProgress] = useState<UpdateProgress>({ phase: "idle" });
  const channel = getUpdateChannel();
  const enabled = isUpdaterEnabled();

  const runCheck = useCallback(async () => {
    if (!enabled) return;
    setProgress({ phase: "checking" });
    try {
      const update = await fetchUpdate();
      if (update) {
        setPending(update);
        setProgress({
          phase: "available",
          version: update.version,
          notes: update.body ?? undefined,
        });
      } else {
        setPending(null);
        setProgress({ phase: "idle" });
      }
    } catch (e) {
      setProgress({ phase: "error", error: String(e) });
    }
  }, [enabled]);

  useEffect(() => {
    getVersion().then(setAppVersion).catch(() => setAppVersion(null));
  }, []);

  useEffect(() => {
    if (autoCheck && enabled) {
      runCheck();
    }
  }, [autoCheck, enabled, runCheck]);

  const install = async () => {
    if (!pending) return;
    try {
      await installUpdate(pending, setProgress);
    } catch (e) {
      setProgress({ phase: "error", error: String(e) });
    }
  };

  useEffect(() => {
    if (progress.phase === "error" && progress.error) {
      logUiError("AppUpdater", progress.error);
    }
  }, [progress]);

  if (!enabled) {
    if (variant === "banner") return null;
    return (
      <section className="updater">
        <h3>Updates</h3>
        {channel === "store" ? (
          <>
            <p className="muted">
              This copy was installed from the <strong>Microsoft Store</strong>.
              Updates are delivered through the Store — open the Store app and check
              Library for updates.
            </p>
            {appVersion && (
              <p className="muted">
                Installed version: <strong>{appVersion}</strong>
              </p>
            )}
          </>
        ) : (
          <>
            <p className="muted">
              Auto-update is enabled in release builds only (not dev mode).
            </p>
            {appVersion && (
              <p className="muted">
                Dev build version: <strong>{appVersion}</strong>
              </p>
            )}
          </>
        )}
      </section>
    );
  }

  if (variant === "banner" && progress.phase !== "available" && !pending) {
    return null;
  }

  const progressLabel =
    progress.phase === "downloading" &&
    progress.contentLength &&
    progress.contentLength > 0
      ? `Downloading ${formatBytes(progress.downloaded ?? 0)} / ${formatBytes(progress.contentLength)}`
      : null;

  if (variant === "banner" && pending) {
    return (
      <div className="update-banner" role="status" aria-live="polite">
        <span>
          Update <strong>v{pending.version}</strong> is available
          {appVersion ? ` (current: v${appVersion})` : ""}.
        </span>
        <div className="update-banner-actions">
          <button className="primary" onClick={install} disabled={progress.phase === "downloading" || progress.phase === "installing"}>
            {progress.phase === "downloading" || progress.phase === "installing"
              ? "Updating…"
              : "Update now"}
          </button>
          <button onClick={() => { setPending(null); setProgress({ phase: "idle" }); }}>
            Later
          </button>
        </div>
        {progressLabel && <span className="muted update-progress">{progressLabel}</span>}
        {progress.phase === "error" && (
          <span className="error">{progress.error}</span>
        )}
      </div>
    );
  }

  return (
    <section className="updater">
      <h3>Updates</h3>
      <p className="muted">
        Installed version: <strong>{appVersion ?? "…"}</strong>
      </p>
      <div className="row">
        <button
          onClick={runCheck}
          disabled={
            progress.phase === "checking" ||
            progress.phase === "downloading" ||
            progress.phase === "installing"
          }
        >
          {progress.phase === "checking" ? "Checking…" : "Check for updates"}
        </button>
        {pending && (
          <button
            className="primary"
            onClick={install}
            disabled={
              progress.phase === "downloading" ||
              progress.phase === "installing"
            }
          >
            Install v{pending.version}
          </button>
        )}
      </div>
      {progress.phase === "available" && pending?.body && (
        <pre className="update-notes">{pending.body}</pre>
      )}
      {progressLabel && <p className="muted">{progressLabel}</p>}
      {progress.phase === "error" && (
        <p className="error">{progress.error}</p>
      )}
      {channel === "github" && (
        <p className="muted">
          Windows installs via NSIS; macOS and Linux update in-app (first macOS install
          is still via DMG). Pacman/AUR installs are updated through your package manager.
        </p>
      )}
    </section>
  );
}
