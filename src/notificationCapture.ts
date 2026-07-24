/** Compare-active flag for milestone notification screenshots. */

export const COMPARE_SESSION_KEY = "wavetrace-compare-active";

let dashboardChartEl: HTMLElement | null = null;
let compareChartEl: HTMLElement | null = null;
let compareRunCount = 0;

export function setDashboardChartEl(el: HTMLElement | null) {
  dashboardChartEl = el;
}

export function setCompareChartEl(el: HTMLElement | null) {
  compareChartEl = el;
}

export function setCompareRunCount(count: number) {
  compareRunCount = count;
}

export function setCompareSessionActive(active: boolean) {
  if (active) {
    sessionStorage.setItem(COMPARE_SESSION_KEY, "1");
  } else {
    sessionStorage.removeItem(COMPARE_SESSION_KEY);
  }
}

export function isCompareSessionActive(): boolean {
  return sessionStorage.getItem(COMPARE_SESSION_KEY) === "1";
}

export function isCompareCapturePreferred(): boolean {
  return isCompareSessionActive();
}

export function getDashboardChartEl(): HTMLElement | null {
  return dashboardChartEl;
}

export function getCompareChartEl(): HTMLElement | null {
  return compareChartEl;
}

export function getCompareRunCount(): number {
  return compareRunCount;
}
