import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

export type UpdatePhase =
  | "idle"
  | "checking"
  | "available"
  | "downloading"
  | "installing"
  | "error";

export interface UpdateProgress {
  phase: UpdatePhase;
  version?: string;
  notes?: string;
  downloaded?: number;
  contentLength?: number;
  error?: string;
}

export function isUpdaterEnabled(): boolean {
  return import.meta.env.PROD;
}

export async function fetchUpdate(): Promise<Update | null> {
  if (!isUpdaterEnabled()) {
    return null;
  }
  return (await check()) ?? null;
}

export async function installUpdate(
  update: Update,
  onProgress: (progress: UpdateProgress) => void
): Promise<void> {
  onProgress({
    phase: "downloading",
    version: update.version,
    notes: update.body ?? undefined,
    downloaded: 0,
    contentLength: 0,
  });

  let downloaded = 0;
  let contentLength = 0;

  await update.downloadAndInstall((event) => {
    switch (event.event) {
      case "Started":
        downloaded = 0;
        contentLength = event.data.contentLength ?? 0;
        onProgress({
          phase: "downloading",
          version: update.version,
          notes: update.body ?? undefined,
          downloaded,
          contentLength,
        });
        break;
      case "Progress":
        downloaded += event.data.chunkLength;
        onProgress({
          phase: "downloading",
          version: update.version,
          notes: update.body ?? undefined,
          downloaded,
          contentLength,
        });
        break;
      case "Finished":
        onProgress({
          phase: "installing",
          version: update.version,
          notes: update.body ?? undefined,
        });
        break;
    }
  });

  await relaunch();
}
