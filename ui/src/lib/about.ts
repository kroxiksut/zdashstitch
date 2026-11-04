import { apiFailure } from './api';

export const ABOUT_ENDPOINT = '/api/about' as const;

/**
 * Build information embedded in the local server (P1-12 §7, P2-00F). The single version source is
 * `[workspace.package] version` in `Cargo.toml`; the UI never reads its own `package.json` version.
 */
export interface AboutInfo {
  version: string;
  /** Full commit hash, or `null` when unknown — shown as `about.field.build_unknown`. */
  build_commit: string | null;
}

export async function loadAbout(fetcher: typeof fetch = fetch): Promise<AboutInfo> {
  const response = await fetcher(ABOUT_ENDPOINT, {
    credentials: 'same-origin',
    headers: { Accept: 'application/json' },
    method: 'GET'
  });
  if (!response.ok) {
    throw await apiFailure(response);
  }
  return (await response.json()) as AboutInfo;
}

/** Short commit for the «Build» field; `null` keeps the «unknown» state. */
export function shortBuildCommit(info: AboutInfo): string | null {
  return info.build_commit === null ? null : info.build_commit.slice(0, 12);
}
