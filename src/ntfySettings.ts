import type { Settings } from "./api";

/** ntfy.sh public server: 2 MB/file, ~20 MB total visitor attachment quota. */
export const NTFY_RECOMMENDED_WAVE_EVERY_WITH_IMAGES = 1000;
export const NTFY_MIN_WAVE_EVERY_WITH_IMAGES = 500;

export function ntfySendsImages(settings: Settings): boolean {
  return Boolean(
    settings.notify_ntfy_enabled && settings.notify_ntfy_attach_capture !== false
  );
}

export function ntfyWaveMilestoneWarning(settings: Settings): string | null {
  if (!ntfySendsImages(settings)) {
    return null;
  }
  const every = settings.notify_wave_every;
  if (every == null) {
    return null;
  }
  if (every < NTFY_MIN_WAVE_EVERY_WITH_IMAGES) {
    return (
      `Wave milestones every ${every.toLocaleString()} waves send many screenshots to ntfy ` +
      `and can hit attachment limits (HTTP 413 on ntfy.sh). Use ${NTFY_MIN_WAVE_EVERY_WITH_IMAGES.toLocaleString()}+ ` +
      `(recommended ${NTFY_RECOMMENDED_WAVE_EVERY_WITH_IMAGES.toLocaleString()}), disable screenshots below, ` +
      `or clear the milestone field.`
    );
  }
  if (every < NTFY_RECOMMENDED_WAVE_EVERY_WITH_IMAGES) {
    return (
      `With screenshots enabled, ${NTFY_RECOMMENDED_WAVE_EVERY_WITH_IMAGES.toLocaleString()} waves ` +
      `between milestones is safer on ntfy.sh's attachment limits.`
    );
  }
  return null;
}
