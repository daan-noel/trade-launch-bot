import { describe, expect, it } from 'vitest';

import {
  fingerprintAutoName,
  fingerprintNameFromGroupKey,
  isGeneratedAutoName,
  isLegacyAutoName,
  isStaleAutoName,
} from './fingerprintNameFromGroupKey';
import { AXES, exactPredicate, type Criteria } from './fingerprintAxes';
import { WILDCARD_NAME } from './types';

const SOL = 1_000_000_000n;

function named(criteria: Criteria, wildcard = false) {
  return fingerprintAutoName({ criteria, wildcard });
}

function window(field: string, min?: string, max?: string) {
  return {
    [field]: { kind: 'window', ...(min != null && { min }), ...(max != null && { max }) },
  };
}

describe('fingerprintAutoName', () => {
  it('emits one chip per configured axis, in registry order', () => {
    expect(
      named({
        prior_launches: exactPredicate('0'),
        cu_limit: exactPredicate('200000'),
        max_cost_lamports: { kind: 'range', min: String(SOL), max: String(2n * SOL) },
        ix_labels: { kind: 'sequence', labels: ['Pump.Fun: Create', 'Pump.Fun: Buy'] },
      }),
    ).toBe('cu_limit=200K · max=1~2 · 2ix:Buy · prior=0');
  });

  it('reads an exact axis, a window and an open-ended bound differently', () => {
    expect(named({ init_buy_lamports: exactPredicate('1515000000') })).toBe('init=1.515');
    expect(named({ init_buy_lamports: { kind: 'range', min: String(2n * SOL) } })).toBe('init=2~');
    expect(named({ init_buy_lamports: { kind: 'range', max: String(2n * SOL) } })).toBe('init=~2');
    expect(named({ ix_count: { kind: 'range', min: '3', max: '5' } })).toBe('ix_count=3~5');
  });

  // The value a float round-trip destroys. It is a real launch setting ("fill at
  // any price"), so it has to name itself exactly rather than be rounded into prose.
  it('names a u64::MAX ceiling exactly', () => {
    expect(named({ max_cost_lamports: exactPredicate('18446744073709551615') })).toBe(
      'max=18446744073.709551615',
    );
  });

  it('names the token set for a wildcard and for a blank draft', () => {
    expect(named({}, true)).toBe(WILDCARD_NAME);
    expect(named({})).toBe(WILDCARD_NAME);
  });

  it('builds the same name from a group key as from the criteria it copies', () => {
    const gk = {
      ...window('max_cost_lamports', String(SOL), String(2n * SOL - 1n)),
      ...window('cu_limit', '200000', '200000'),
    };
    expect(fingerprintNameFromGroupKey(gk)).toBe('cu_limit=200K · max=1~1.999999999');
  });

  it('skips a key value that names nothing a rule can match on', () => {
    const gk = {
      max_cost_lamports: { kind: 'missing' },
      token_program_id: { kind: 'text', value: 'Tokenkeg' },
      ...window('cu_limit', '1', '1'),
    };
    expect(fingerprintNameFromGroupKey(gk)).toBe('cu_limit=1');
  });
});

describe('the auto-name grammar', () => {
  // The property that lets a naming change finish: everything the generator emits,
  // the grammar recognises — so a stored name that drifts is rewritten, and a
  // nickname never is. Walks the whole registry, so a new axis is covered.
  it('recognises every name it emits, for every axis', () => {
    for (const def of AXES) {
      const preds =
        def.kind === 'sequence'
          ? [{ kind: 'sequence' as const, labels: ['Pump.Fun: Create', 'System Program: Transfer'] }]
          : [
              exactPredicate('0'),
              exactPredicate('1515000000'),
              exactPredicate('18446744073709551615'),
              { kind: 'range' as const, min: '1', max: '200000' },
              { kind: 'range' as const, min: '3' },
              { kind: 'range' as const, max: '3' },
            ];
      for (const pred of preds) {
        const name = named({ [def.id]: pred });
        expect(isGeneratedAutoName(name), `${def.id} emits ${name}`).toBe(true);
        expect(isStaleAutoName(name, name), `${name} must be stable`).toBe(false);
      }
    }
  });

  it('treats an unrecognised part as a nickname, so a real name is never rewritten', () => {
    expect(isGeneratedAutoName('8dtx router')).toBe(false);
    expect(isGeneratedAutoName('cu_limit=200K · hand-written')).toBe(false);
    expect(isStaleAutoName('8dtx router', 'cu_limit=200K')).toBe(false);
  });

  it('rewrites a generated name that no longer matches the axes', () => {
    expect(isStaleAutoName('cu_limit=999K', 'cu_limit=200K')).toBe(true);
  });

  // The retired width chip is not in the current grammar, so a name carrying one
  // would be frozen as a nickname forever unless it is named as retired.
  it('heals a name carrying the retired bucket-width chip', () => {
    expect(isLegacyAutoName('init=1 · bkt=0.5')).toBe(true);
    expect(isLegacyAutoName('init=1 · bkt=exact')).toBe(true);
    expect(isStaleAutoName('init=1 · bkt=0.5', 'init=1~1.999999999')).toBe(true);
    // A nickname sitting beside a retired chip is still a nickname.
    expect(isLegacyAutoName('8dtx · bkt=0.5')).toBe(false);
  });

  it('recognises the other retired shapes and a blank', () => {
    for (const n of ['', '  ', 'flow-discovery bind', 'sweep 12ab · group 3', 'c · x', 'f · y', 's · z']) {
      expect(isLegacyAutoName(n), n).toBe(true);
    }
    expect(isLegacyAutoName('8dtx router')).toBe(false);
  });

  it('accepts the wildcard name', () => {
    expect(isGeneratedAutoName(WILDCARD_NAME)).toBe(true);
  });
});
