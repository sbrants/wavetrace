export type AppTab = "dashboard" | "history" | "settings";

let setTabFn: ((tab: AppTab) => void) | null = null;
let getTabFn: (() => AppTab) | null = null;

export function registerTabControl(
  setTab: (tab: AppTab) => void,
  getTab: () => AppTab,
) {
  setTabFn = setTab;
  getTabFn = getTab;
}

export function isTabControlReady(): boolean {
  return setTabFn != null && getTabFn != null;
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => window.setTimeout(resolve, ms));
}

async function waitForPaint(): Promise<void> {
  await new Promise<void>((resolve) => {
    window.requestAnimationFrame(() => {
      window.requestAnimationFrame(() => resolve());
    });
  });
}

async function waitForTabControl(maxMs = 8000): Promise<void> {
  const start = Date.now();
  while (Date.now() - start < maxMs) {
    if (isTabControlReady()) {
      return;
    }
    await sleep(50);
  }
  throw new Error("Tab control is not registered");
}

export async function waitForSelector(
  selector: string,
  maxMs = 6000,
): Promise<HTMLElement | null> {
  const start = Date.now();
  while (Date.now() - start < maxMs) {
    const el = document.querySelector<HTMLElement>(selector);
    if (el) {
      return el;
    }
    await sleep(100);
  }
  return null;
}

/** Switch tab for capture; returns the tab to restore later. */
export async function beginTabCapture(
  tab: AppTab,
  renderDelayMs = 1200,
): Promise<AppTab> {
  await waitForTabControl();
  const previous = getTabFn!();
  setTabFn!(tab);
  await waitForPaint();
  window.dispatchEvent(new Event("resize"));
  await sleep(renderDelayMs);
  return previous;
}

export function restoreTab(tab: AppTab) {
  if (!setTabFn) {
    return;
  }
  setTabFn(tab);
}
