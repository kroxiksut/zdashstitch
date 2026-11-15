import { describe, expect, it } from 'vitest';

import { canSavePreviewedPage, samePagePreviewScope, type PagePreviewScope } from './page-preview-guard';

function deferred<T>(): { promise: Promise<T>; resolve(value: T): void } {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}

describe('selected Workcopy page preview guard', () => {
  it('ignores a late A preview after B is selected and accepts only the B preview', async () => {
    let current: PagePreviewScope = { workcopyId: 'wc-one', pageId: 'A', generation: 1 };
    let previewed: PagePreviewScope | null = null;
    const a = deferred<boolean>();
    const aScope = current;
    const aRequest = a.promise.then((ready) => {
      if (samePagePreviewScope(aScope, current) && ready) previewed = aScope;
    });
    current = { workcopyId: 'wc-one', pageId: 'B', generation: 2 };
    a.resolve(true);
    await aRequest;
    expect(canSavePreviewedPage(previewed, current, true)).toBe(false);

    const b = deferred<boolean>();
    const bScope = current;
    const bRequest = b.promise.then((ready) => {
      if (samePagePreviewScope(bScope, current) && ready) previewed = bScope;
    });
    b.resolve(true);
    await bRequest;
    expect(canSavePreviewedPage(previewed, current, true)).toBe(true);
  });

  it('rejects a late result when the details view changes to another Workcopy', async () => {
    const oldScope: PagePreviewScope = { workcopyId: 'wc-one', pageId: 'A', generation: 3 };
    const newScope: PagePreviewScope = { workcopyId: 'wc-two', pageId: 'A', generation: 4 };
    expect(samePagePreviewScope(oldScope, newScope)).toBe(false);
    expect(canSavePreviewedPage(oldScope, newScope, true)).toBe(false);
  });
});
