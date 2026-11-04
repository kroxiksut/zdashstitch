import { describe, expect, it, vi } from 'vitest';

import { ABOUT_ENDPOINT, loadAbout, shortBuildCommit } from './about';

describe('build information (P2-00F)', () => {
  it('reads version and commit from the local server, not from package.json', async () => {
    const commit = 'a'.repeat(40);
    const fetcher = vi.fn(
      async () =>
        new Response(JSON.stringify({ version: '0.0.0', build_commit: commit }), {
          status: 200,
          headers: { 'Content-Type': 'application/json' }
        })
    );

    const info = await loadAbout(fetcher);

    expect(fetcher).toHaveBeenCalledWith(ABOUT_ENDPOINT, {
      credentials: 'same-origin',
      headers: { Accept: 'application/json' },
      method: 'GET'
    });
    expect(info.version).toBe('0.0.0');
    expect(shortBuildCommit(info)).toBe('a'.repeat(12));
  });

  it('keeps an unknown commit unknown', () => {
    expect(shortBuildCommit({ version: '0.0.0', build_commit: null })).toBeNull();
  });
});
