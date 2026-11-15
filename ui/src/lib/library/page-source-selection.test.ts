import { describe, expect, it } from 'vitest';
import { choosePageSource } from './page-source-selection';

describe('asynchronous dashboard page sources', () => {
  it('keeps a library choice when the empty draft list arrives later', () => {
    let selected = choosePageSource('', ['lp-first']);
    selected = choosePageSource(selected, []);
    expect(selected).toBe('lp-first');
  });

  it('selects a library page when the empty draft list arrives first', () => {
    let selected = choosePageSource('', []);
    selected = choosePageSource(selected, ['lp-first']);
    expect(selected).toBe('lp-first');
  });

  it('preserves a user choice when the other response arrives', () => {
    let selected = choosePageSource('', ['pg-first', 'pg-second']);
    selected = 'pg-second';
    selected = choosePageSource(selected, ['lp-first']);
    expect(selected).toBe('pg-second');
  });
});
