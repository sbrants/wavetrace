import { useCallback, useState } from "react";

const STORAGE_PREFIX = "wavetrace.";

/** Like useState<boolean>, but the value survives an app restart via localStorage. */
export function usePersistedBoolean(
  key: string,
  defaultValue: boolean
): [boolean, (value: boolean | ((prev: boolean) => boolean)) => void] {
  const storageKey = STORAGE_PREFIX + key;
  const [value, setValue] = useState<boolean>(() => {
    try {
      const raw = localStorage.getItem(storageKey);
      return raw === null ? defaultValue : raw === "true";
    } catch {
      return defaultValue;
    }
  });

  const setPersisted = useCallback(
    (next: boolean | ((prev: boolean) => boolean)) => {
      setValue((prev) => {
        const resolved = typeof next === "function" ? next(prev) : next;
        try {
          localStorage.setItem(storageKey, String(resolved));
        } catch {
          // localStorage unavailable (private mode, quota, etc.) — value still
          // works for this session, it just won't survive a restart.
        }
        return resolved;
      });
    },
    [storageKey]
  );

  return [value, setPersisted];
}
