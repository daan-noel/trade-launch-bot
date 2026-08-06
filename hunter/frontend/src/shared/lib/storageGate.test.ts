import { describe, expect, it } from 'vitest';

/**
 * The "one gate" rule, enforced instead of documented: app preferences go
 * through `lib/storage` (or `hooks/useLocalStorage`), never a raw
 * `localStorage.*` call in a component. A second, ad-hoc accessor is how the
 * key registry drifted out of truth in the first place — a key nothing in
 * `STORAGE_KEYS` knows about, in a namespace the cleanup never purges.
 *
 * Add to the allowlist only for storage that is NOT a user preference, and say
 * why here.
 */
const ALLOWLIST = [
  // The gate itself.
  '/src/shared/lib/storage.ts',
  // Ephemeral cross-tab claim stamp (which tab owns a desktop notification), not
  // a preference — it must not survive as a pref and has no registry entry.
  '/src/live/lib/desktopNotify.ts',
];

/** `import.meta.glob` — Vite's build-time source inliner. Typed locally because
 *  the app's tsconfig doesn't pull in `vite/client`. */
type GlobFn = (
  pattern: string,
  options: { query: string; import: string; eager: true },
) => Record<string, string>;

const SOURCES = (import.meta as unknown as { glob: GlobFn }).glob(
  '/src/**/*.{ts,tsx}',
  { query: '?raw', import: 'default', eager: true },
);

/** Source with comments stripped — prose that merely *mentions* localStorage
 *  (the hooks' doc blocks do, deliberately) is not a call site. */
function code(text: string): string {
  return text.replace(/\/\*[\s\S]*?\*\//g, '').replace(/(^|[^:])\/\/.*$/gm, '$1');
}

function offenders(pattern: RegExp): string[] {
  return Object.entries(SOURCES)
    .filter(([path]) => !/\.test\.tsx?$/.test(path) && !ALLOWLIST.includes(path))
    .filter(([, text]) => pattern.test(code(text)))
    .map(([path]) => path)
    .sort();
}

describe('localStorage gate', () => {
  it('has no raw localStorage access outside the allowlist', () => {
    expect(offenders(/\blocalStorage\s*\./)).toEqual([]);
  });

  it('has no leftover `hunter.` key literals', () => {
    expect(offenders(/['"`]hunter\./)).toEqual([]);
  });
});
