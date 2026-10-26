export type ThemePreference = 'system' | 'light' | 'dark';
export type ResolvedTheme = Exclude<ThemePreference, 'system'>;

interface ThemeRoot {
  dataset: { theme?: string };
  style: { colorScheme: string };
}

interface MediaQueryLike {
  readonly matches: boolean;
  addEventListener(type: 'change', listener: () => void): void;
  removeEventListener(type: 'change', listener: () => void): void;
}

export function resolveTheme(
  preference: ThemePreference,
  prefersDark: boolean
): ResolvedTheme {
  return preference === 'system' ? (prefersDark ? 'dark' : 'light') : preference;
}

export function applyTheme(
  root: ThemeRoot,
  preference: ThemePreference,
  prefersDark: boolean
): ResolvedTheme {
  root.dataset.theme = preference;
  const resolved = resolveTheme(preference, prefersDark);
  root.style.colorScheme = resolved;
  return resolved;
}

export function createThemeController(
  root: ThemeRoot,
  media: MediaQueryLike,
  initial: ThemePreference
): {
  setPreference(preference: ThemePreference): ResolvedTheme;
  destroy(): void;
} {
  let preference = initial;
  const refresh = () => applyTheme(root, preference, media.matches);
  media.addEventListener('change', refresh);
  refresh();
  return {
    setPreference(next) {
      preference = next;
      return refresh();
    },
    destroy() {
      media.removeEventListener('change', refresh);
    }
  };
}
