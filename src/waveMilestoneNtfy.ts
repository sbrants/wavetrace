import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { api } from "./api";
import { blobToBase64, captureChartCard, waitForChartReady } from "./chartScreenshot";
import { isCompareCapturePreferred } from "./notificationCapture";
import {
  beginTabCapture,
  restoreTab,
  waitForSelector,
  type AppTab,
} from "./tabCapture";
import { logUiError } from "./uiError";

interface WaveMilestoneNtfyPayload {
  title: string;
  body: string;
  preferCompare: boolean;
}

let handling = false;
const queue: WaveMilestoneNtfyPayload[] = [];
const inFlight = new Set<string>();

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => window.setTimeout(resolve, ms));
}

function shouldUseCompare(payload: WaveMilestoneNtfyPayload): boolean {
  return payload.preferCompare || isCompareCapturePreferred();
}

async function prepareUiForCapture(useCompare: boolean): Promise<AppTab> {
  await api.prepareMainWindowForCapture();
  await sleep(600);

  const tab = useCompare ? "history" : "dashboard";
  const previous = await beginTabCapture(tab, useCompare ? 1800 : 1200);

  if (useCompare) {
    const compareCard = await waitForSelector(
      ".history .chart-card.compare-card",
      5000,
    );
    if (!compareCard) {
      throw new Error("Compare card not found after switching to History");
    }
    compareCard.scrollIntoView({ block: "start", behavior: "instant" });
    await sleep(600);
    window.dispatchEvent(new Event("resize"));
    await sleep(500);
  }

  return previous;
}

async function captureUiChartPng(useCompare: boolean): Promise<string | null> {
  const selector = useCompare
    ? ".history .chart-card.compare-card"
    : ".dashboard .chart-card";
  const el = document.querySelector<HTMLElement>(selector);
  if (!el) {
    void api.appendAppLog(
      "waveMilestoneNtfy",
      `chart element missing for selector ${selector}`,
    );
    return null;
  }
  try {
    await waitForChartReady(el, 6000);
    const blob = await captureChartCard(el);
    return blobToBase64(blob);
  } catch (e) {
    logUiError("waveMilestoneNtfy.chart", e);
    void api.appendAppLog(
      "waveMilestoneNtfy",
      `chart capture failed: ${e instanceof Error ? e.message : String(e)}`,
    );
    return null;
  }
}

function payloadKey(payload: WaveMilestoneNtfyPayload): string {
  return `${payload.title}\0${payload.body}`;
}

async function handleWaveMilestone(payload: WaveMilestoneNtfyPayload) {
  const key = payloadKey(payload);
  if (inFlight.has(key)) {
    return;
  }
  inFlight.add(key);
  let previousTab: AppTab = "dashboard";
  let uiPngBase64: string | null = null;
  try {
    const useCompare = shouldUseCompare(payload);
    void api.appendAppLog(
      "waveMilestoneNtfy",
      `prepare (preferCompare=${payload.preferCompare}, useCompare=${useCompare})`,
    );

    try {
      previousTab = await prepareUiForCapture(useCompare);
      uiPngBase64 = await captureUiChartPng(useCompare);
      void api.appendAppLog(
        "waveMilestoneNtfy",
        uiPngBase64
          ? `chart captured (${uiPngBase64.length} b64 chars)`
          : "chart capture returned null",
      );
    } catch (e) {
      logUiError("waveMilestoneNtfy.prepare", e);
      void api.appendAppLog(
        "waveMilestoneNtfy",
        `prepare failed: ${e instanceof Error ? e.message : String(e)}`,
      );
    }

    await api.completeWaveMilestoneNtfy(uiPngBase64);
    void api.appendAppLog("waveMilestoneNtfy", "complete_wave_milestone_ntfy ok");
  } finally {
    restoreTab(previousTab);
    inFlight.delete(key);
  }
}

async function drainQueue() {
  if (handling) {
    return;
  }
  handling = true;
  try {
    while (queue.length > 0) {
      const payload = queue.shift()!;
      try {
        await handleWaveMilestone(payload);
      } catch (e) {
        logUiError("waveMilestoneNtfy.publish", e);
        void api.appendAppLog(
          "waveMilestoneNtfy",
          `publish failed: ${e instanceof Error ? e.message : String(e)}`,
        );
      }
    }
  } finally {
    handling = false;
  }
}

function onWaveMilestone(payload: WaveMilestoneNtfyPayload) {
  queue.push(payload);
  void drainQueue();
}

let unlisten: UnlistenFn | null = null;

async function ensureListener(): Promise<void> {
  if (unlisten) {
    return;
  }
  unlisten = await listen<WaveMilestoneNtfyPayload>(
    "wave-milestone-ntfy",
    (event) => onWaveMilestone(event.payload),
  );
}

void ensureListener();

if (import.meta.hot) {
  import.meta.hot.dispose(() => {
    void unlisten?.();
    unlisten = null;
  });
}
