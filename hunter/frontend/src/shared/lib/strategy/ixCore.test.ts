import { describe, expect, it } from 'vitest';

// ONE copy of the vectors, read straight from the engine crate (twin:
// `the_shared_core_fixture` in hunter/engine/src/metrics/trade_keys.rs).
import fixture from '../../../../../engine/fixtures/ix_core_parity.json';

import { coreKey, coreLabels, coreMarks, coreMarksFromText, coreMarksText } from './ixCore';

interface CoreCase {
  name: string;
  labels: string[];
  core: string[];
  marks: string;
}

const { cases } = fixture as unknown as { cases: CoreCase[] };

describe('ixCore matches the shared core fixture', () => {
  it('loads the fixture the Rust suite asserts', () => {
    expect(cases.length).toBeGreaterThan(0);
  });

  for (const c of cases) {
    it(c.name, () => {
      expect(coreLabels(c.labels)).toEqual(c.core);
      expect(coreMarksText(coreMarks(c.labels))).toBe(c.marks);
      expect(coreMarksFromText(c.marks)).toBe(coreMarks(c.labels));
      expect(coreKey(c.core)).toBe(coreKey(c.labels));
    });
  }

  it('reads marks in any order and refuses what the engine refuses', () => {
    expect(coreMarksFromText('A3 CP CL T1')).toBe(coreMarksFromText('CL CP T1 A3'));
    expect(coreMarksFromText('CU')).toBeNull();
    expect(coreMarksFromText('A256')).toBeNull();
  });
});
