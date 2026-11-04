/**
 * Interface theme preference as stored in the settings file. `high_contrast` is the in-application
 * light-based high-contrast theme (owner decision O-038); the operating-system contrast mode
 * (`forced-colors`) still takes priority over every theme in CSS.
 */
export type ThemePreference = 'system' | 'light' | 'dark' | 'high_contrast';
export type ResolvedTheme = Exclude<ThemePreference, 'system'>;
export const THEME_PREFERENCES: ReadonlyArray<ThemePreference> = ['system', 'light', 'dark', 'high_contrast'];

/** Reduced motion as stored in the settings file: `system` follows `prefers-reduced-motion`. */
export type MotionPreference = 'system' | 'reduce' | 'no-preference';

interface ThemeRoot {
  dataset: { theme?: string; motion?: string };
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

/** `color-scheme` of a resolved theme: the high-contrast theme is light-based. */
export function colorSchemeOf(theme: ResolvedTheme): 'light' | 'dark' {
  return theme === 'dark' ? 'dark' : 'light';
}

export function applyTheme(
  root: ThemeRoot,
  preference: ThemePreference,
  prefersDark: boolean
): ResolvedTheme {
  root.dataset.theme = preference;
  const resolved = resolveTheme(preference, prefersDark);
  root.style.colorScheme = colorSchemeOf(resolved);
  return resolved;
}

/**
 * Explicit reduced motion goes to `data-motion`; `system` removes it so the CSS media query decides.
 * Only the attribute changes: route, focus, selection and form data stay as they are.
 */
export function applyMotion(root: Pick<ThemeRoot, 'dataset'>, preference: MotionPreference): void {
  if (preference === 'system') delete root.dataset.motion;
  else root.dataset.motion = preference;
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
