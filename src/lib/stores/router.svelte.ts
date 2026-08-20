/**
 * Navigation.
 *
 * Grimoire has three destinations, so it has a route *state* rather than a
 * router library: a URL scheme would buy nothing in a desktop app with no
 * addressable links, and a dependency would be pure ceremony.
 *
 * The last location is remembered across launches so reopening the app returns
 * the writer to the Page they left, which is the single most useful thing a
 * writing tool can do at startup.
 */

export type Route =
  | { name: 'library' }
  | { name: 'workspace'; volumeId: string; pageId: string | null }
  | { name: 'settings' };

const STORAGE_KEY = 'grimoire.route';

function restore(): Route {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return { name: 'library' };
    const parsed = JSON.parse(raw) as Route;
    // Only the workspace is worth restoring; landing in Settings on launch
    // would be disorienting, and the Library is the natural default.
    if (parsed?.name === 'workspace' && typeof parsed.volumeId === 'string') {
      return { name: 'workspace', volumeId: parsed.volumeId, pageId: parsed.pageId ?? null };
    }
  } catch {
    // A corrupted or unreadable value is not worth a failed launch.
  }
  return { name: 'library' };
}

class Router {
  current = $state<Route>({ name: 'library' });
  /** Where to return to when Settings is dismissed. */
  private previous: Route = { name: 'library' };

  start(): void {
    if (typeof localStorage === 'undefined') return;
    this.current = restore();
  }

  go(route: Route): void {
    if (this.current.name !== 'settings') this.previous = this.current;
    this.current = route;
    this.persist();
  }

  toLibrary(): void {
    this.go({ name: 'library' });
  }

  toWorkspace(volumeId: string, pageId: string | null = null): void {
    this.go({ name: 'workspace', volumeId, pageId });
  }

  toSettings(): void {
    this.go({ name: 'settings' });
  }

  /** Leaves Settings, returning to whatever was open before it. */
  closeSettings(): void {
    this.current = this.previous;
    this.persist();
  }

  /** Records the open Page without treating it as a navigation event. */
  setPage(pageId: string | null): void {
    if (this.current.name !== 'workspace') return;
    this.current = { ...this.current, pageId };
    this.persist();
  }

  private persist(): void {
    if (typeof localStorage === 'undefined') return;
    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify(this.current));
    } catch {
      // Losing the remembered location costs one extra click next launch.
    }
  }
}

export const router = new Router();
