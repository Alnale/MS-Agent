export function loadNumber(key: string, fallback: number): number {
  try {
    const val = localStorage.getItem(key);
    return val ? parseFloat(val) : fallback;
  } catch {
    return fallback;
  }
}

export function loadBool(key: string, fallback: boolean): boolean {
  try {
    return localStorage.getItem(key) === 'true';
  } catch {
    return fallback;
  }
}

export function loadString(key: string, fallback: string): string {
  try {
    return localStorage.getItem(key) || fallback;
  } catch {
    return fallback;
  }
}

export function loadFromStorage<T>(key: string, fallback: T): T {
  try {
    const raw = localStorage.getItem(key);
    if (raw === null) return fallback;
    return JSON.parse(raw) as T;
  } catch {
    return fallback;
  }
}
