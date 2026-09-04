import { describe, expect, it } from 'vitest';
import {
  isLaunchGrain,
  parseGrainIds,
  templateGrain,
  templateProgram,
  toggleWorkingTemplate,
  workingListHits,
} from './templateGrain';

describe('templateGrain', () => {
  it('matches the Rust harvest working list spellings', () => {
    expect(
      templateGrain([
        'Compute Budget: SetComputeUnitLimit',
        'Compute Budget: SetComputeUnitPrice',
        'Associated Token: CreateIdempotent',
        'Axiom Trade: Buy',
        'System Program: Transfer',
      ]),
    ).toBe('Axiom Trade|CU|ATA|F');
    expect(
      templateGrain([
        'Compute Budget: SetComputeUnitLimit',
        'Associated Token: Create',
        'Axiom Trade: Buy',
        'System Program: AdvanceNonceAccount',
        'System Program: Transfer',
      ]),
    ).toBe('Axiom Trade|CU|ATA|N|F');
    expect(
      templateGrain([
        'Compute Budget: SetComputeUnitPrice',
        'Bloom Router: Swap',
        'System Program: Transfer',
      ]),
    ).toBe('Bloom Router|CU|F');
    expect(
      templateGrain([
        'Compute Budget: SetComputeUnitLimit',
        'Bloom: Buy',
        'System Program: Transfer',
      ]),
    ).toBe('Bloom|CU|F');
  });

  it('names Photon / Terminal / GMGN the same way the catalog does', () => {
    const cuAtaF = (prog: string) =>
      templateGrain([
        'Compute Budget: SetComputeUnitLimit',
        'Associated Token: CreateIdempotent',
        `${prog}: Buy`,
        'System Program: Transfer',
      ]);
    expect(cuAtaF('Photon')).toBe('Photon|CU|ATA|F');
    expect(cuAtaF('Terminal')).toBe('Terminal|CU|ATA|F');
    expect(cuAtaF('GMGN Bot')).toBe('GMGN Bot|CU|ATA|F');
    expect(cuAtaF('GMGN')).toBe('GMGN|CU|ATA|F');
  });

  it('keeps Pump.Fun buys off the launch grain', () => {
    expect(templateGrain(['Pump.Fun: Buy'])).toBe('Pump.Fun');
    expect(templateGrain(['Pump.Fun: CreateIdempotent', 'Pump.Fun: Buy'])).toBe('launch');
    expect(isLaunchGrain(['Pump.Fun: CreateIdempotent', 'Pump.Fun: Buy'])).toBe(true);
    expect(isLaunchGrain(['Pump.Fun: Buy'])).toBe(false);
  });

  it('empty and all-boilerplate still spell a grain, never throw', () => {
    expect(templateGrain([])).toBe('(direct)');
    expect(
      templateGrain([
        'Compute Budget: SetComputeUnitLimit',
        'Associated Token: Create',
        'System Program: Transfer',
      ]),
    ).toBe('(direct)|CU|ATA|F');
  });
});

describe('parseGrainIds / toggleWorkingTemplate', () => {
  it('splits paste on commas and newlines, drops blanks and dupes', () => {
    expect(parseGrainIds('Axiom Trade|CU|ATA|F\nPhoton|CU|ATA|F, Axiom Trade|CU|ATA|F\n')).toEqual([
      'Axiom Trade|CU|ATA|F',
      'Photon|CU|ATA|F',
    ]);
  });

  it('toggles membership', () => {
    const a = 'Axiom Trade|CU|ATA|F';
    expect(toggleWorkingTemplate([], a)).toEqual([a]);
    expect(toggleWorkingTemplate([a], a)).toEqual([]);
    expect(toggleWorkingTemplate([a, 'Photon|CU|ATA|F'], a)).toEqual(['Photon|CU|ATA|F']);
    expect(toggleWorkingTemplate([a], '  ')).toEqual([a]);
  });

  it('accepts a bare program name', () => {
    expect(parseGrainIds('Axiom Trade')).toEqual(['Axiom Trade']);
    expect(toggleWorkingTemplate([], 'Axiom Trade')).toEqual(['Axiom Trade']);
  });

  it('hits a print when the list names the grain or the program', () => {
    const labels = [
      'Associated Token: CreateIdempotent',
      'Axiom Trade: Buy',
      'System Program: Transfer',
    ];
    expect(templateProgram(labels)).toBe('Axiom Trade');
    expect(templateGrain(labels)).toBe('Axiom Trade|ATA|F');
    expect(workingListHits(new Set(['Axiom Trade|ATA|F']), labels)).toBe(true);
    expect(workingListHits(new Set(['Axiom Trade']), labels)).toBe(true);
    expect(workingListHits(new Set(['Photon|CU|ATA|F']), labels)).toBe(false);
  });
});
