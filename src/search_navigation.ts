export async function loadUntilFound(
  found: () => boolean,
  hasOlder: () => boolean,
  count: () => number,
  loadOlder: () => Promise<void>,
): Promise<void> {
  while (!found() && hasOlder()) {
    const previousCount = count();
    await loadOlder();
    if (count() <= previousCount) return;
  }
}
