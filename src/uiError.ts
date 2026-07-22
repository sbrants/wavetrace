import { invoke } from "@tauri-apps/api/core";

import { showToast } from "./toast";

export function formatUiError(error: unknown): string {
  if (error instanceof Error) return error.message;
  return String(error);
}

let logQueue = Promise.resolve();

/** Append a UI error to wavetrace.log (fire-and-forget). */
export function logUiError(source: string, error: unknown): void {
  const message = formatUiError(error);
  logQueue = logQueue
    .then(() => invoke<void>("append_app_log", { source, message }))
    .catch(() => {});
}

/** Log to wavetrace.log, then show an in-app toast (unless alert: false). */
export function reportUiError(
  error: unknown,
  source: string,
  options?: { alert?: boolean },
): string {
  const message = formatUiError(error);
  logUiError(source, message);
  if (options?.alert !== false) {
    showToast(message);
  }
  return message;
}
