import { defineConfig } from 'vitest/config';
import tsconfigPaths from 'vite-tsconfig-paths';

// Minimal test runner config. `tsconfigPaths` gives tests the same `components/…`,
// `services/…` path aliases the app builds with. Node environment — the shared
// evaluators are pure logic, no DOM. Dev-only; never part of the shipped build.
export default defineConfig({
  plugins: [tsconfigPaths()],
  test: {
    include: ['src/**/*.test.ts'],
    environment: 'node',
  },
});
