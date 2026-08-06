import { beforeEach, describe, expect, it, vi } from 'vitest';

/**
 * `lib/storage` is the ONE gate for durable UI preferences. These lock the three
 * invariants a drifting call site would break: every registered key is `mt:`,
 * a blob field write never disturbs its siblings, and the one-shot migration
 * folds every retired key into its current home before purging it.
 *
 * The module migrates at import time, so each case seeds a fresh fake store and
 * re-imports (`vi.resetModules`) instead of calling the migration by hand.
 */

class FakeStorage {
  private map = new Map<string, string>();
  get length(): number {
    return this.map.size;
  }
  key(i: number): string | null {
    return [...this.map.keys()][i] ?? null;
  }
  getItem(k: string): string | null {
    return this.map.get(k) ?? null;
  }
  setItem(k: string, v: string): void {
    this.map.set(k, v);
  }
  removeItem(k: string): void {
    this.map.delete(k);
  }
}

let store: FakeStorage;

/** Seed the store, then import a fresh copy of the module (migration runs once). */
async function loadStorage(seed: Record<string, string> = {}) {
  store = new FakeStorage();
  for (const [k, v] of Object.entries(seed)) store.setItem(k, v);
  vi.stubGlobal('localStorage', store);
  vi.resetModules();
  return import('./storage');
}

beforeEach(() => {
  vi.unstubAllGlobals();
});

describe('STORAGE_KEYS registry', () => {
  it('namespaces every durable key under mt:', async () => {
    const { STORAGE_KEYS } = await loadStorage();
    for (const [name, key] of Object.entries(STORAGE_KEYS)) {
      expect(key, `${name} must be mt:-prefixed`).toMatch(/^mt:/);
    }
  });

  it('has no duplicate key values', async () => {
    const { STORAGE_KEYS } = await loadStorage();
    const values = Object.values(STORAGE_KEYS);
    expect(new Set(values).size).toBe(values.length);
  });
});

describe('blob fields', () => {
  it('writes one field without disturbing its siblings', async () => {
    const { getField, setField } = await loadStorage();
    setField('mt:blob', 'a', 1);
    setField('mt:blob', 'b', 'two');
    setField('mt:blob', 'a', 3);
    expect(getField('mt:blob', 'a', 0)).toBe(3);
    expect(getField('mt:blob', 'b', '')).toBe('two');
  });

  it('falls back when the blob or the field is absent', async () => {
    const { getField } = await loadStorage();
    expect(getField('mt:missing', 'a', 'dflt')).toBe('dflt');
  });

  it('keeps a stored `false` instead of treating it as absent', async () => {
    const { getField, setField } = await loadStorage();
    setField('mt:blob', 'open', false);
    expect(getField('mt:blob', 'open', true)).toBe(false);
  });
});

describe('migrateLegacyStorage', () => {
  it('folds the creation-stats flats into one blob and purges them', async () => {
    const { STORAGE_KEYS, getJSON } = await loadStorage({
      'mt:dashboard.metric': '"count"',
      'mt:dashboard.grouped.top': '16',
    });
    expect(getJSON(STORAGE_KEYS.pageCreationStats, {})).toEqual({
      metric: 'count',
      groupedTop: 16,
    });
    expect(store.getItem('mt:dashboard.metric')).toBeNull();
    expect(store.getItem('mt:dashboard.grouped.top')).toBeNull();
  });

  it('moves the hunter.* namespace into the registry and purges it', async () => {
    const { STORAGE_KEYS, getJSON } = await loadStorage({
      'hunter.lab.flowDiscovery.config': '{"top":5}',
      'hunter.pnlDistDensity': 'sparse',
    });
    expect(getJSON(STORAGE_KEYS.flowDiscoveryConfig, null)).toEqual({ top: 5 });
    expect(getJSON(STORAGE_KEYS.pnlDistDensity, null)).toBe('sparse');
    expect(store.getItem('hunter.lab.flowDiscovery.config')).toBeNull();
    expect(store.getItem('hunter.pnlDistDensity')).toBeNull();
  });

  it("turns the per-accordion '0'/'1' keys into ids in the shared map", async () => {
    const { ACCORDION_IDS, STORAGE_KEYS, getField } = await loadStorage({
      'mt:inspect-detail-open': '1',
      'mt:metric-selector-open': '0',
    });
    expect(getField(STORAGE_KEYS.uiAccordion, ACCORDION_IDS.inspectDetail, null)).toBe(true);
    expect(getField(STORAGE_KEYS.uiAccordion, ACCORDION_IDS.metricSelector, null)).toBe(false);
    expect(store.getItem('mt:inspect-detail-open')).toBeNull();
  });

  it('folds stable table-charts flags into the map and drops volatile ones', async () => {
    const { getTableCharts } = await loadStorage({
      'mt:tablecharts:tokens': '1',
      'mt:tablecharts:simulate-positions-run42': '1',
    });
    expect(getTableCharts('tokens', false)).toBe(true);
    expect(getTableCharts('simulate-positions-run42', false)).toBe(false);
    expect(store.getItem('mt:tablecharts:tokens')).toBeNull();
  });

  it('renames the unprefixed filter mirrors', async () => {
    const { STORAGE_KEYS, getString } = await loadStorage({
      'simulate.modeFilter': 'paper',
    });
    expect(getString(`${STORAGE_KEYS.filterMode}.simulate`)).toBe('paper');
    expect(store.getItem('simulate.modeFilter')).toBeNull();
  });

  it('purges pre-namespace flats and leaves third-party keys alone', async () => {
    await loadStorage({
      tpsl_rules_cols: '["a"]',
      sweep_cfg_window: '30',
      app_timezone: 'UTC',
      'some-vendor-key': 'keep me',
    });
    expect(store.getItem('tpsl_rules_cols')).toBeNull();
    expect(store.getItem('sweep_cfg_window')).toBeNull();
    expect(store.getItem('app_timezone')).toBeNull();
    expect(store.getItem('some-vendor-key')).toBe('keep me');
  });

  it('never overwrites a value the user has already set under the new key', async () => {
    const { STORAGE_KEYS, getJSON } = await loadStorage({
      'mt:dashboard.metric': '"count"',
      'mt:page.creationStats': '{"metric":"trades"}',
    });
    expect(getJSON<{ metric: string }>(STORAGE_KEYS.pageCreationStats, { metric: '' }).metric).toBe(
      'trades',
    );
  });
});
