export type RovingKey = 'ArrowLeft' | 'ArrowRight' | 'ArrowUp' | 'ArrowDown' | 'Home' | 'End';

export function nextRovingIndex(
  current: number,
  count: number,
  key: string,
  orientation: 'horizontal' | 'vertical' | 'both' = 'both'
): number {
  if (count <= 0) return -1;
  if (key === 'Home') return 0;
  if (key === 'End') return count - 1;
  const previous = key === 'ArrowLeft' || key === 'ArrowUp';
  const next = key === 'ArrowRight' || key === 'ArrowDown';
  if (!previous && !next) return current;
  if (orientation === 'horizontal' && (key === 'ArrowUp' || key === 'ArrowDown')) return current;
  if (orientation === 'vertical' && (key === 'ArrowLeft' || key === 'ArrowRight')) return current;
  return (current + (previous ? -1 : 1) + count) % count;
}
