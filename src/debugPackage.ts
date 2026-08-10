import { api, type DebugScreenshot } from "./api";
import { logUiError } from "./uiError";

export type DebugTab = "dashboard" | "history" | "settings";

const DEBUG_CAPTURE_EVENT = "wavetrace-debug-capture";
const DEBUG_CAPTURE_READY = "wavetrace-debug-tab-ready";
const DEBUG_READY_TIMEOUT_MS = 10_000;

function waitForDebugReady(): Promise<void> {
  return new Promise((resolve, reject) => {
    const timer = window.setTimeout(() => {
      window.removeEventListener(DEBUG_CAPTURE_READY, handler);
      reject(new Error("Timed out waiting for the UI to switch tabs."));
    }, DEBUG_READY_TIMEOUT_MS);
    const handler = () => {
      window.clearTimeout(timer);
      window.removeEventListener(DEBUG_CAPTURE_READY, handler);
      resolve();
    };
    window.addEventListener(DEBUG_CAPTURE_READY, handler);
  });
}

function dispatchDebugCapture(detail: {
  phase: "start" | "switch" | "end";
  tab?: DebugTab;
}) {
  window.dispatchEvent(
    new CustomEvent(DEBUG_CAPTURE_EVENT, { detail }),
  );
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => window.setTimeout(resolve, ms));
}

const DEBUG_TABS: { tab: DebugTab; label: string }[] = [
  { tab: "dashboard", label: "dashboard" },
  { tab: "history", label: "history" },
  { tab: "settings", label: "settings" },
];

/** Show a tab briefly, run `fn`, then restore the tab the user had open. */
export async function withTabVisible<T>(
  tab: DebugTab,
  fn: () => Promise<T>,
  renderDelayMs = 400,
): Promise<T> {
  // Listen before dispatch so a sync/rAF ready event cannot be missed.
  let ready = waitForDebugReady();
  dispatchDebugCapture({ phase: "start" });
  await ready;
  try {
    ready = waitForDebugReady();
    dispatchDebugCapture({ phase: "switch", tab });
    await ready;
    await sleep(renderDelayMs);
    return await fn();
  } finally {
    ready = waitForDebugReady();
    dispatchDebugCapture({ phase: "end" });
    await ready;
  }
}

/** Switch tabs, capture the app window on each, then restore the prior tab. */
export async function captureDebugScreenshots(): Promise<DebugScreenshot[]> {
  const shots: DebugScreenshot[] = [];
  for (const { tab, label } of DEBUG_TABS) {
    try {
      await withTabVisible(tab, async () => {
        const png_base64 = await api.captureAppWindow();
        shots.push({ label, png_base64 });
      });
    } catch (e) {
      logUiError(`debugPackage.screenshot.${label}`, e);
    }
  }
  return shots;
}
