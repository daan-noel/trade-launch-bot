import { describe, expect, it } from 'vitest';

// The real file, not a fixture: this test is the format guard for the Inventory page.
import md from '../../../../docs/plans/strategies/_!___inventory.md?raw';

import { STATUSES, parseInventory } from './inventory';

describe('parseInventory on _!___inventory.md', () => {
  const inv = parseInventory(md);
  const ideas = inv.slots.flatMap((s) => s.families.flatMap((f) => f.ideas));

  it('reads every slot in order', () => {
    expect(inv.slots.map((s) => s.code)).toEqual(['D', 'E', 'P', 'X', 'R', 'S']);
  });

  it('reads every idea row the file holds', () => {
    // Every seven-column table row outside the header and separator lines is an idea.
    const rowCount = md
      .split(/\r?\n/)
      .filter((l) => l.startsWith('| **') && l.split(' | ').length === 7).length;
    expect(ideas.length).toBe(rowCount);
    expect(ideas.length).toBeGreaterThan(50);
  });

  it('gives every idea a known status, a name and an example', () => {
    for (const i of ideas) {
      expect(STATUSES).toContain(i.status);
      expect(i.name).not.toMatch(/\*/);
      expect(i.example.length).toBeGreaterThan(0);
    }
  });

  it('keeps idea names unique, so each table row has its own key', () => {
    const names = ideas.map((i) => i.name);
    expect(new Set(names).size).toBe(names.length);
  });

  it('reads the structure grid, and every cell names a row that exists', () => {
    const names = new Set(ideas.map((i) => i.name));
    expect(inv.grid.columns).toEqual(['whole list', 'each structure', 'landing together']);
    expect(inv.grid.rows.length).toBeGreaterThan(5);
    for (const r of inv.grid.rows) {
      expect(r.cells.length).toBe(inv.grid.columns.length);
      for (const c of r.cells) expect(names.has(c.name)).toBe(true);
    }
  });

  it('points every Old names entry at a row that exists', () => {
    // "Nothing leaves silently": each bold name in the Old names table must still be a row.
    const section = md.replace(/\r\n/g, '\n').split('\n## Old names\n')[1] ?? '';
    const names = new Set(ideas.map((i) => i.name));
    const targets = [...section.matchAll(/\*\*([^*]+)\*\*/g)].map((m) => m[1]);
    expect(section.split('\n').filter((l) => l.startsWith('| ') && !l.startsWith('| ---')).length).toBeGreaterThan(200);
    expect(targets.filter((t) => !names.has(t))).toEqual([]);
  });
});
