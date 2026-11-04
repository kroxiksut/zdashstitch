import { describe, expect, it, vi } from 'vitest';

import { THEME_PREFERENCES, applyMotion, applyTheme, colorSchemeOf, createThemeController, resolveTheme } from './theme';

describe('theme runtime', () => {
  it('resolves system without changing explicit preferences', () => {
    expect(resolveTheme('system', true)).toBe('dark');
    expect(resolveTheme('system', false)).toBe('light');
    expect(resolveTheme('light', true)).toBe('light');
  });

  it('changes only theme attributes and preserves application state', () => {
    const root = { dataset: {}, style: { colorScheme: '' } };
    const applicationState = { selection: 'widget-7', dirty: true, formValue: 'unchanged' };
    const snapshot = structuredClone(applicationState);

    applyTheme(root, 'dark', false);

    expect(root).toEqual({ dataset: { theme: 'dark' }, style: { colorScheme: 'dark' } });
    expect(applicationState).toEqual(snapshot);
  });

  it('reacts to operating-system changes only in system mode', () => {
    let listener: () => void = () => {};
    const media = {
      matches: false,
      addEventListener: vi.fn((_type: 'change', next: () => void) => {
        listener = next;
      }),
      removeEventListener: vi.fn()
    };
    const root = { dataset: {}, style: { colorScheme: '' } };
    const controller = createThemeController(root, media, 'system');
    expect(root.style.colorScheme).toBe('light');

    Object.defineProperty(media, 'matches', { value: true });
    listener();
    expect(root.style.colorScheme).toBe('dark');

    controller.setPreference('light');
    listener();
    expect(root.style.colorScheme).toBe('light');
    controller.destroy();
    expect(media.removeEventListener).toHaveBeenCalledOnce();
  });
});

describe('high contrast and reduced motion (O-038, P2-00G)', () => {
  it('applies the light-based high-contrast theme without following the system', () => {
    const root = { dataset: {}, style: { colorScheme: '' } };
    expect(applyTheme(root, 'high_contrast', true)).toBe('high_contrast');
    expect(root).toEqual({ dataset: { theme: 'high_contrast' }, style: { colorScheme: 'light' } });
    expect(colorSchemeOf('dark')).toBe('dark');
    expect(THEME_PREFERENCES).toEqual(['system', 'light', 'dark', 'high_contrast']);
  });

  it('sets an explicit motion choice and leaves «system» to the media query', () => {
    const root: { dataset: { motion?: string } } = { dataset: {} };
    applyMotion(root, 'reduce');
    expect(root.dataset.motion).toBe('reduce');
    applyMotion(root, 'no-preference');
    expect(root.dataset.motion).toBe('no-preference');
    applyMotion(root, 'system');
    expect('motion' in root.dataset).toBe(false);
  });
});
