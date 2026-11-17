/**
 * Refresh behaviour of a view that reloads when it opens: a Synchronization that settles while the
 * view is the open one reloads it at once; otherwise it only remembers that the data is stale, so
 * `refreshIfStale` can catch up when the user returns.
 */
export function createRefreshWhenActive(options: { isActive(): boolean; refresh(): Promise<void> }) {
  let stale = false;

  function refresh(): Promise<void> {
    stale = false;
    return options.refresh();
  }

  return {
    refresh,
    refreshIfActive(): Promise<void> {
      if (options.isActive()) return refresh();
      stale = true;
      return Promise.resolve();
    },
    refreshIfStale: (): Promise<void> => (stale ? refresh() : Promise.resolve()),
  };
}
