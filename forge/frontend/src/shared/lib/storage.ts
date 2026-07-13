/** Tiny typed localStorage helpers (used by the token chart's prefs). */

export const STORAGE_KEYS = {
  chartPrefs: 'forge.tokenChart.prefs',
} as const;

export function getString(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

export function setString(key: string, value: string): void {
  try {
    localStorage.setItem(key, value);
  } catch {
    /* ignore (private mode / quota) */
  }
}
