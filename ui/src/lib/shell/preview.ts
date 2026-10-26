// Page preview geometry in Zabbix 7.4 grid proportions (P1-01B §2a):
// 72 columns across the preview width, row height 70 × preview width / screen width,
// expressed as a CSS aspect ratio so no element has to be measured at run time.

import type { DraftWidget } from './drafts';

export const GRID_COLUMNS = 72;
export const GRID_ROWS = 64;
export const ZABBIX_ROW_HEIGHT_PX = 70;
export const SCREEN_WIDTHS = [1366, 1920, 2560] as const;
export type ScreenWidth = (typeof SCREEN_WIDTHS)[number];
export const DEFAULT_SCREEN_WIDTH: ScreenWidth = 1920;

/** Rows occupied by the page content, clamped to the Zabbix grid. */
export function rowsUsed(widgets: ReadonlyArray<Pick<DraftWidget, 'y' | 'height'>>): number {
  const bottom = widgets.reduce((max, widget) => Math.max(max, widget.y + widget.height), 0);
  return Math.min(GRID_ROWS, Math.max(0, bottom));
}

/**
 * CSS aspect ratio of the preview area: 72 columns span the chosen screen width and every row is
 * 70 px of that screen, so the preview keeps Zabbix proportions at any rendered width.
 */
export function previewAspectRatio(screenWidth: number, rows: number): string | undefined {
  if (!(screenWidth > 0) || !(rows > 0)) return undefined;
  return `${screenWidth} / ${rows * ZABBIX_ROW_HEIGHT_PX}`;
}

export interface WidgetBox {
  /** Percent of the preview width. */
  readonly left: number;
  readonly width: number;
  /** Percent of the preview height (the rows shown). */
  readonly top: number;
  readonly height: number;
}

export function widgetBox(widget: Pick<DraftWidget, 'x' | 'y' | 'width' | 'height'>, rows: number): WidgetBox {
  const shown = rows > 0 ? rows : 1;
  return {
    left: (widget.x / GRID_COLUMNS) * 100,
    width: (widget.width / GRID_COLUMNS) * 100,
    top: (widget.y / shown) * 100,
    height: (widget.height / shown) * 100
  };
}

export type PeriodUnit = 'second' | 'minute' | 'hour';

/** Express a page display period in the largest whole unit, for `Intl.NumberFormat` unit formatting. */
export function periodUnit(seconds: number): { value: number; unit: PeriodUnit } {
  if (seconds >= 3600 && seconds % 3600 === 0) return { value: seconds / 3600, unit: 'hour' };
  if (seconds >= 60 && seconds % 60 === 0) return { value: seconds / 60, unit: 'minute' };
  return { value: seconds, unit: 'second' };
}
