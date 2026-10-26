import { render } from 'svelte/server';
import { describe, expect, it } from 'vitest';

import { createI18n } from '../i18n';
import Button from './Button.svelte';
import DataTable from './DataTable.svelte';
import TextField from './TextField.svelte';
import { nextRovingIndex } from './keyboard';

const i18n = createI18n('en');

describe('common component semantics', () => {
  it('renders a native busy button that remains focusable', () => {
    const { body } = render(Button, { props: { i18n, label: { id: 'startup.loading' }, busy: true } });
    expect(body).toContain('<button');
    expect(body).toContain('aria-busy="true"');
    expect(body).not.toContain('disabled');
    expect(body).toContain('Checking the local backend');
  });

  it('associates field error text without discarding the value', () => {
    const { body } = render(TextField, {
      props: {
        i18n,
        id: 'endpoint',
        label: { id: 'startup.status.backend' },
        value: 'user supplied value',
        error: { id: 'startup.error.unavailable' }
      }
    });
    expect(body).toContain('aria-invalid="true"');
    expect(body).toContain('endpoint-error');
    expect(body).toContain('user supplied value');
  });

  it('uses native table semantics and preserves technical values', () => {
    const { body } = render(DataTable, {
      props: {
        i18n,
        caption: { id: 'startup.status.label' },
        headers: [{ id: 'startup.status.backend' }],
        rows: [{ id: 'one', cells: [{ value: 'item.key[param]', kind: 'technical' }] }]
      }
    });
    expect(body).toContain('<table>');
    expect(body).toContain('<th scope="col">');
    expect(body).toContain('item.key[param]');
  });

  it('implements wrapping APG roving focus keys', () => {
    expect(nextRovingIndex(0, 3, 'ArrowLeft', 'horizontal')).toBe(2);
    expect(nextRovingIndex(2, 3, 'ArrowRight', 'horizontal')).toBe(0);
    expect(nextRovingIndex(1, 3, 'Home')).toBe(0);
    expect(nextRovingIndex(1, 3, 'End')).toBe(2);
    expect(nextRovingIndex(1, 3, 'ArrowDown', 'horizontal')).toBe(1);
  });
});
