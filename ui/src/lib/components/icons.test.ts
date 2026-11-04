import { render } from 'svelte/server';
import { describe, expect, it } from 'vitest';

import Icon from './Icon.svelte';
import { ICON_PATHS, type IconName } from './icons';

// The vendored upstream files, read as text at test time only (never bundled into the app).
const VENDORED = import.meta.glob<string>('../../../../assets/icons/tabler/outline/*.svg', {
  query: '?raw',
  import: 'default',
  eager: true
});

function vendoredPaths(name: string): string[] {
  const svg = VENDORED[`../../../../assets/icons/tabler/outline/${name}.svg`];
  expect(svg, name).toBeTypeOf('string');
  expect(svg).not.toMatch(/<script|\bon[a-z]+=|href=|url\(/i);
  return [...svg.matchAll(/<path d="([^"]*)"/g)].map((match) => match[1]);
}

describe('Tabler outline icons (O-024)', () => {
  it('keeps every runtime icon identical to the vendored upstream file', () => {
    for (const name of Object.keys(ICON_PATHS) as IconName[]) {
      expect(ICON_PATHS[name], name).toEqual(vendoredPaths(name));
    }
  });

  it('renders a decorative inline SVG without raw HTML', () => {
    const { body } = render(Icon, { props: { name: 'circle-check' } });
    expect(body).toContain('aria-hidden="true"');
    expect(body).toContain('d="M9 12l2 2l4 -4"');
  });
});
