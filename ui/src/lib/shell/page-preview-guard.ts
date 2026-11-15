export interface PagePreviewScope {
  readonly workcopyId: string;
  readonly pageId: string;
  readonly generation: number;
}

export function samePagePreviewScope(expected: PagePreviewScope, current: PagePreviewScope): boolean {
  return expected.workcopyId === current.workcopyId
    && expected.pageId === current.pageId
    && expected.generation === current.generation;
}

export function canSavePreviewedPage(
  previewed: PagePreviewScope | null,
  current: PagePreviewScope,
  ready: boolean
): boolean {
  return ready && previewed !== null && samePagePreviewScope(previewed, current);
}
