/**
 * Boolean onboarding flags live in localStorage. Storage can be unavailable
 * (private mode, disabled quota), so both helpers swallow failures and fall
 * back to the in-memory default.
 */
export function readFlag(key: string): boolean {
  try {
    return localStorage.getItem(key) === "1";
  } catch {
    return false;
  }
}

export function writeFlag(key: string, value: boolean): void {
  try {
    if (value) localStorage.setItem(key, "1");
    else localStorage.removeItem(key);
  } catch {
    /* storage is optional */
  }
}
