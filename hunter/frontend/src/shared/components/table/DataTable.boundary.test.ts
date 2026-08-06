import { describe, expect, it } from 'vitest';

/**
 * Architecture guard (no-DB, runs on every `npm test`) — the SAME spirit as the
 * backend SSOT/parity guards.
 *
 * `components/table/` is the PURE, general-purpose table primitive. Token concerns
 * live one layer up in `components/tokens/` (`TokenTable`, `sharedTokenColumns`,
 * `TokensFilterBar`), and the dependency is strictly one-way: `tokens/` → `table/`,
 * never the reverse. This test fails if any file in `table/` imports the token
 * layer or an app-specific (`@live`/`@lab`) tree — i.e. if token vocabulary starts
 * leaking back into the primitive. Keep `DataTable` reusable for ANY row type.
 */

// Raw source of every sibling file in `table/`, loaded via Vite's glob (vitest runs
// through Vite). Keeps the guard dependency-free — no `node:fs`, no `@types/node`.
const sources = (
  import.meta as unknown as {
    glob(
      pattern: string,
      opts: { eager: true; query: string; import: string },
    ): Record<string, string>;
  }
).glob('./*.{ts,tsx}', { eager: true, query: '?raw', import: 'default' });

/** Sources a `table/` file must never import. */
const FORBIDDEN = [
  { label: 'the token layer (components/tokens)', re: /(^|\/)tokens(\/|$)/ },
  { label: 'the live app tree (@live)', re: /^@live(\/|$)/ },
  { label: 'the lab app tree (@lab)', re: /^@lab(\/|$)/ },
];

/** Every import/`export … from` specifier in a source file. */
function importSpecifiers(src: string): string[] {
  const out: string[] = [];
  const re = /(?:import|export)[^'"]*?\bfrom\s*['"]([^'"]+)['"]|import\s*['"]([^'"]+)['"]/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(src)) !== null) out.push(m[1] ?? m[2]);
  return out;
}

// [relativePath, rawSource] for every non-test file in the directory.
const files = Object.entries(sources).filter(([path]) => !path.endsWith('.test.ts'));

describe('DataTable primitive stays token-agnostic', () => {
  it('scans the whole table/ directory', () => {
    // Guard against a glob mistake vacuously passing the checks below.
    expect(files.length).toBeGreaterThan(0);
  });

  it.each(files)('%s imports nothing from the token layer', (path, src) => {
    for (const spec of importSpecifiers(src)) {
      for (const { label, re } of FORBIDDEN) {
        expect(
          re.test(spec),
          `${path} imports "${spec}" from ${label} — the table primitive must stay general-purpose; put token concerns in components/tokens/ instead.`,
        ).toBe(false);
      }
    }
  });
});
