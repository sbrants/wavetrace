/** Compare-active flag for milestone notification screenshots. */

export const COMPARE_SESSION_KEY = "wavetrace-compare-active";

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
