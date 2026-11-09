/** Local draft history is memory-only; a saved checkpoint does not reset it. */
export class EditorHistory<T> {
  private current: T;
  private past: Array<{ value: T; action: string }> = [];
  private future: Array<{ value: T; action: string }> = [];
  truncated = false;

  constructor(value: T, private readonly limit = 200) { this.current = structuredClone(value); }
  get undoAction(): string | null { return this.past.at(-1)?.action ?? null; }
  get redoAction(): string | null { return this.future.at(-1)?.action ?? null; }
  commit(value: T, action: string): boolean {
    if (JSON.stringify(value) === JSON.stringify(this.current)) return false;
    this.past.push({ value: this.current, action });
    if (this.past.length > this.limit) { this.past.shift(); this.truncated = true; }
    this.current = structuredClone(value); this.future = [];
    return true;
  }
  undo(): T | null {
    const entry = this.past.pop(); if (!entry) return null;
    this.future.push({ value: this.current, action: entry.action }); this.current = entry.value;
    return structuredClone(this.current);
  }
  redo(): T | null {
    const entry = this.future.pop(); if (!entry) return null;
    this.past.push({ value: this.current, action: entry.action }); this.current = entry.value;
    return structuredClone(this.current);
  }
}
