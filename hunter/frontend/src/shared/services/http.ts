/**
 * Shared low-level fetch wrapper for the imperative (non-RTK-Query) service
 * calls. Throws an `Error` carrying the backend's `{ error: "..." }` message on
 * a non-2xx response, and resolves `204 No Content` to `undefined`.
 *
 * Lives in `shared/services` so both mode builds' service files (rule CRUD,
 * lifecycle, sweeps, swings) reuse one implementation.
 */
export async function request<T>(url: string, init?: RequestInit): Promise<T> {
  const resp = await fetch(url, init);
  if (!resp.ok) {
    const body = await resp.json().catch(() => null);
    const msg =
      body && typeof body === 'object' && 'error' in body
        ? String((body as { error: string }).error)
        : `HTTP ${resp.status}`;
    throw new Error(msg);
  }
  if (resp.status === 204) return undefined as T;
  return resp.json() as Promise<T>;
}
