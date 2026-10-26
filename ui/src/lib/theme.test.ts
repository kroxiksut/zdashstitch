import { describe, expect, it, vi } from 'vitest';

import { applyTheme, createThemeController, resolveTheme } from './theme';

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
