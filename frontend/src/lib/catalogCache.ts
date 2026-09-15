// A per-tab stale-while-refresh cache makes catalog navigation and reloads
// immediate without leaving account metadata behind after the browser tab closes.
export function readCatalogCache<T>(key: string): T | null {
  try {
    const value = sessionStorage.getItem(`catalog:${key}`);
    return value ? JSON.parse(value) as T : null;
  } catch {
    return null;
  }
}

export function writeCatalogCache(key: string, value: unknown): void {
  try {
    sessionStorage.setItem(`catalog:${key}`, JSON.stringify(value));
  } catch {
    // Storage can be disabled or full; network loading remains functional.
  }
}
