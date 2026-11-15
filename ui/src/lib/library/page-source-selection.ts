/** Keep an explicit choice while asynchronous source lists arrive in either order. */
export function choosePageSource(current: string, arrivedIds: readonly string[]): string {
  return current || arrivedIds[0] || '';
}
