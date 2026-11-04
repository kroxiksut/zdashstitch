// Client-side routes of the application shell (P1-01B §3, P1-01H §2).
// Route parameters are opaque local identifiers only: never paths, endpoints,
// secrets, Zabbix IDs or user-visible names.

export const DRAFT_SECTIONS = ['overview', 'structure', 'bindings', 'checks', 'publication'] as const;
export type DraftSection = (typeof DRAFT_SECTIONS)[number];

export const SETTINGS_SECTIONS = [
  'general',
  'appearance',
  'workspace',
  'editor',
  'connections',
  'security',
  'diagnostics',
  'about'
] as const;
export type SettingsSection = (typeof SETTINGS_SECTIONS)[number];

export const DEFAULT_SETTINGS_SECTION: SettingsSection = 'appearance';

export type Route =
  | { readonly name: 'start' }
  | { readonly name: 'drafts' }
  | { readonly name: 'server'; readonly connectionLocalId: string | null }
  // P1-01H §2: the copy check (mockup 03-03). The chosen dashboard lives in memory only, never in
  // the address: a reload returns to the dashboard list.
  | { readonly name: 'server_copy'; readonly connectionLocalId: string }
  // P2-05E: read-only preview of the chosen server dashboard; like the copy check, the dashboard
  // lives in memory only (no Zabbix ID in the address), so a reload returns to the list.
  | { readonly name: 'server_preview'; readonly connectionLocalId: string }
  | { readonly name: 'library' }
  | { readonly name: 'connections' }
  // P2-01G: editor and check of one profile (mockups 05-02, 05-03 breadcrumbs). `localId` is the
  // opaque profile ID, never an address or a name.
  | { readonly name: 'connection_new' }
  | { readonly name: 'connection_edit'; readonly localId: string }
  | { readonly name: 'connection_check'; readonly localId: string }
  | { readonly name: 'settings'; readonly section: SettingsSection }
  | { readonly name: 'draft'; readonly localId: string; readonly section: DraftSection };

export interface ParsedRoute {
  readonly route: Route;
  /** Set when the address should be replaced by its canonical form. */
  readonly canonical?: string;
}

const LOCAL_ID = /^[A-Za-z0-9][A-Za-z0-9_-]{0,127}$/;

export function isLocalId(value: string): boolean {
  return LOCAL_ID.test(value);
}

function segments(pathname: string): string[] | null {
  const parts = pathname.split('/').filter((part) => part.length > 0);
  try {
    return parts.map((part) => decodeURIComponent(part));
  } catch {
    return null;
  }
}

function isMember<T extends string>(values: readonly T[], value: string | undefined): value is T {
  return value !== undefined && (values as readonly string[]).includes(value);
}

/** Parse a pathname. Unknown addresses resolve to the start screen with a canonical replacement. */
export function parseRoute(pathname: string): ParsedRoute {
  const start: ParsedRoute = { route: { name: 'start' }, canonical: '/' };
  const parts = segments(pathname);
  if (parts === null) return start;
  const [head, second, third, ...rest] = parts;
  if (head === undefined) return { route: { name: 'start' } };
  switch (head) {
    case 'drafts':
      return second === undefined ? { route: { name: 'drafts' } } : { route: { name: 'drafts' }, canonical: '/drafts' };
    case 'library':
      return second === undefined ? { route: { name: 'library' } } : { route: { name: 'library' }, canonical: '/library' };
    case 'connections':
      if (second === undefined) return { route: { name: 'connections' } };
      if (second === 'new' && third === undefined) return { route: { name: 'connection_new' } };
      if (isLocalId(second) && rest.length === 0) {
        if (third === 'edit') return { route: { name: 'connection_edit', localId: second } };
        if (third === 'check') return { route: { name: 'connection_check', localId: second } };
      }
      return { route: { name: 'connections' }, canonical: '/connections' };
    case 'server':
      if (second === undefined) return { route: { name: 'server', connectionLocalId: null } };
      if (isLocalId(second) && third === 'dashboards' && rest.length === 0) {
        return { route: { name: 'server', connectionLocalId: second } };
      }
      if (isLocalId(second) && third === 'copy' && rest.length === 0) {
        return { route: { name: 'server_copy', connectionLocalId: second } };
      }
      if (isLocalId(second) && third === 'preview' && rest.length === 0) {
        return { route: { name: 'server_preview', connectionLocalId: second } };
      }
      return { route: { name: 'server', connectionLocalId: null }, canonical: '/server' };
    case 'settings':
      if (isMember(SETTINGS_SECTIONS, second) && third === undefined) {
        return { route: { name: 'settings', section: second } };
      }
      return {
        route: { name: 'settings', section: DEFAULT_SETTINGS_SECTION },
        canonical: settingsPath(DEFAULT_SETTINGS_SECTION)
      };
    case 'draft': {
      if (second === undefined || !isLocalId(second)) return start;
      if (third === undefined) {
        return { route: { name: 'draft', localId: second, section: 'overview' }, canonical: draftPath(second, 'overview') };
      }
      if (!isMember(DRAFT_SECTIONS, third)) {
        return { route: { name: 'draft', localId: second, section: 'overview' }, canonical: draftPath(second, 'overview') };
      }
      // `/structure/*` also covers the page editor boundary (P1-01H); the editor is not available yet.
      if (rest.length > 0 && third !== 'structure') {
        return { route: { name: 'draft', localId: second, section: third }, canonical: draftPath(second, third) };
      }
      return { route: { name: 'draft', localId: second, section: third } };
    }
    default:
      return start;
  }
}

export function draftPath(localId: string, section: DraftSection = 'overview'): string {
  return `/draft/${encodeURIComponent(localId)}/${section}`;
}

export function settingsPath(section: SettingsSection): string {
  return `/settings/${section}`;
}

export function connectionPath(localId: string, view: 'edit' | 'check'): string {
  return `/connections/${encodeURIComponent(localId)}/${view}`;
}

export function serverPath(connectionLocalId: string | null): string {
  return connectionLocalId === null ? '/server' : `/server/${encodeURIComponent(connectionLocalId)}/dashboards`;
}

export function serverCopyPath(connectionLocalId: string): string {
  return `/server/${encodeURIComponent(connectionLocalId)}/copy`;
}

export function serverPreviewPath(connectionLocalId: string): string {
  return `/server/${encodeURIComponent(connectionLocalId)}/preview`;
}

export type TopSection = 'start' | 'drafts' | 'server' | 'library' | 'connections' | 'settings';

/** The global header item that owns a route; an open draft belongs to «Drafts». */
export function topSection(route: Route): TopSection {
  switch (route.name) {
    case 'draft':
      return 'drafts';
    case 'server_copy':
    case 'server_preview':
      return 'server';
    case 'connection_new':
    case 'connection_edit':
    case 'connection_check':
      return 'connections';
    default:
      return route.name;
  }
}

export function sameRoute(left: Route, right: Route): boolean {
  return JSON.stringify(left) === JSON.stringify(right);
}

/** Decide whether a click on a link may be handled by the shell router instead of the browser. */
export function internalLinkPath(
  anchor: Pick<HTMLAnchorElement, 'href' | 'target' | 'hasAttribute'>,
  origin: string,
  event: Pick<MouseEvent, 'button' | 'metaKey' | 'ctrlKey' | 'shiftKey' | 'altKey' | 'defaultPrevented'>
): string | null {
  if (event.defaultPrevented || event.button !== 0) return null;
  if (event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return null;
  if ((anchor.target && anchor.target !== '_self') || anchor.hasAttribute('download')) return null;
  let url: URL;
  try {
    url = new URL(anchor.href);
  } catch {
    return null;
  }
  if (url.origin !== origin || url.pathname.startsWith('/api/') || url.pathname === '/api') return null;
  // In-page anchors such as the skip link keep native behaviour; shell routes never use fragments.
  if (url.hash !== '' || url.search !== '') return null;
  return url.pathname;
}
