import { defineConfig } from 'vitest/config';
import tsconfigPaths from 'vite-tsconfig-paths';

// Minimal test runner config. `tsconfigPaths` gives tests the same `components/…`,
// `services/…` path aliases the app builds with. Node environment — the shared
// evaluators are pure logic, no DOM. Dev-only; never part of the shipped build.
export default defineConfig({
  // `tsconfig.test.json`, not the default lookup: the build's `tsconfig.json`
  // excludes test files, and this plugin only maps aliases for files a project
  // includes. The test project adds them back.
  plugins: [tsconfigPaths({ projects: ['tsconfig.test.json'] })],
  test: {
    include: ['src/**/*.test.ts'],
    environment: 'node',
  },
});
