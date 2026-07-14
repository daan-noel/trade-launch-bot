// ESLint flat config — module-boundary enforcement only (Part 4 / M14).
//
// The frontend is one shared core plus two mode trees (live, lab) built
// separately (build-time mode split — no runtime capability advertisement). These
// zones stop the trees from importing across the split, which the design forbids:
//
//   src/shared/  ⊬  @live/*, @lab/*   — the shared core must stay mode-agnostic
//   src/live/    ⊬  @lab/*            — live ships to EC2 without any lab code
//   src/lab/     ⊬  @live/*           — lab has no keys/trader (sellToken is live-only)
//
// This is intentionally NOT a general lint pass — its whole job is to CI-gate the
// boundaries. Run with `npm run lint`.
//
// The `@typescript-eslint` and `react-hooks` plugins are *registered* (so the many
// existing `// eslint-disable-next-line <rule>` comments resolve to a known rule)
// but their rules are left OFF — turning them on is out of scope for a boundary gate.
import tseslint from 'typescript-eslint';
import reactHooks from 'eslint-plugin-react-hooks';

/** Build a `no-restricted-imports` rule config that bans a set of alias globs. */
const banAliases = (aliases, message) => ({
  'no-restricted-imports': [
    'error',
    { patterns: [{ group: aliases.flatMap((a) => [a, `${a}/**`]), message }] },
  ],
});

export default tseslint.config(
  { ignores: ['dist/**', 'node_modules/**', '*.config.ts', '*.config.base.ts', '*.config.js'] },
  {
    files: ['src/**/*.{ts,tsx}'],
    // Existing inline eslint-disable comments name these rules; register the plugins
    // so those directives resolve, but don't enable (or flag as unused) any rule.
    plugins: { '@typescript-eslint': tseslint.plugin, 'react-hooks': reactHooks },
    linterOptions: { reportUnusedDisableDirectives: 'off' },
    languageOptions: {
      parser: tseslint.parser,
      parserOptions: { ecmaFeatures: { jsx: true }, sourceType: 'module' },
    },
  },
  {
    files: ['src/shared/**/*.{ts,tsx}'],
    rules: banAliases(
      ['@live', '@lab'],
      'Boundary violation: src/shared must not import from @live or @lab — keep the shared core mode-agnostic.',
    ),
  },
  {
    files: ['src/live/**/*.{ts,tsx}'],
    rules: banAliases(
      ['@lab'],
      'Boundary violation: src/live must not import from @lab — the live bundle ships to EC2 without lab code.',
    ),
  },
  {
    files: ['src/lab/**/*.{ts,tsx}'],
    rules: banAliases(
      ['@live'],
      'Boundary violation: src/lab must not import from @live — lab has no keys/trader (e.g. sellToken is live-only).',
    ),
  },
);
