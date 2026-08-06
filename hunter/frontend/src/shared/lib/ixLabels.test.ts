import { describe, expect, it } from 'vitest';
import {
  configuredIxLabels,
  formatIxLabelsText,
  isIxLabelJsonFilter,
  ixLabelAction,
  ixLabelsActions,
  ixLabelsCountTail,
  ixLabelsMatchFilter,
  parseIxLabelFilter,
  parseIxLabelsText,
} from './ixLabels';

describe('configuredIxLabels — mirrors Rust configured_labels', () => {
  it('folds the two spellings of "not set" onto null', () => {
    expect(configuredIxLabels([])).toBeNull();
    expect(configuredIxLabels(null)).toBeNull();
    expect(configuredIxLabels(undefined)).toBeNull();
  });

  it('passes a real label list through unchanged', () => {
    const labels = ['Pump.Fun: Create', 'Pump.Fun: Buy'];
    expect(configuredIxLabels(labels)).toBe(labels);
    expect(configuredIxLabels([''])).toEqual(['']); // non-empty list; parse layer trims
  });
});

describe('formatIxLabelsText', () => {
  it('pretty-prints a JSON array', () => {
    expect(formatIxLabelsText(['a', 'b'])).toBe('[\n  "a",\n  "b"\n]');
  });

  it('handles null/empty', () => {
    expect(formatIxLabelsText(null)).toBe('');
    expect(formatIxLabelsText([])).toBe('');
  });
});

describe('ixLabelAction / ixLabelsActions / ixLabelsCountTail', () => {
  const A = ['Pump.Fun: Create_v2', 'Associated Token: Create', 'Pump.Fun: Buy'];
  const B = ['Pump.Fun: Create_v2', 'Associated Token: Create', 'Pump.Fun: BuyExactSolIn'];

  it('takes the action past the LAST ": "', () => {
    expect(ixLabelAction('Pump.Fun: Create_v2')).toBe('Create_v2');
    expect(ixLabelAction('Vote: Program: Withdraw')).toBe('Withdraw');
    expect(ixLabelAction('Undecorated')).toBe('Undecorated');
  });

  it('joins actions in on-chain order', () => {
    expect(ixLabelsActions(A)).toBe('Create_v2 > Create > Buy');
    expect(ixLabelsActions([])).toBe('');
  });

  it('separates same-length sets that a bare count conflates', () => {
    expect(A.length).toBe(B.length); // the bug: both render `3ix`
    expect(ixLabelsCountTail(A)).toBe('3ix:Buy');
    expect(ixLabelsCountTail(B)).toBe('3ix:BuyExactSolIn');
    expect(ixLabelsCountTail(A)).not.toBe(ixLabelsCountTail(B));
  });

  it('falls back to the bare count when there is no tail action', () => {
    expect(ixLabelsCountTail([])).toBe('0ix');
    expect(ixLabelsCountTail(['Pump.Fun: Buy', '  '])).toBe('2ix');
  });
});

describe('parseIxLabelsText', () => {
  it('returns null for empty', () => {
    expect(parseIxLabelsText('')).toEqual({ labels: null, error: null });
    expect(parseIxLabelsText('  \n  ')).toEqual({ labels: null, error: null });
  });

  it('parses pretty JSON', () => {
    expect(parseIxLabelsText('[\n  "Pump.Fun: Create",\n  "Pump.Fun: Buy"\n]')).toEqual({
      labels: ['Pump.Fun: Create', 'Pump.Fun: Buy'],
      error: null,
    });
  });

  it('parses compact JSON', () => {
    expect(parseIxLabelsText('["Pump.Fun: Create", "Pump.Fun: Buy"]')).toEqual({
      labels: ['Pump.Fun: Create', 'Pump.Fun: Buy'],
      error: null,
    });
  });

  it('parses legacy one-per-line paste', () => {
    expect(parseIxLabelsText('Pump.Fun: Create\nPump.Fun: Buy')).toEqual({
      labels: ['Pump.Fun: Create', 'Pump.Fun: Buy'],
      error: null,
    });
  });

  it('parses legacy comma-separated paste', () => {
    expect(parseIxLabelsText('create, buy')).toEqual({
      labels: ['create', 'buy'],
      error: null,
    });
  });

  it('errors on invalid JSON when text starts with [', () => {
    expect(parseIxLabelsText('[oops')).toEqual({ labels: null, error: 'Invalid JSON' });
  });

  it('errors on non-string JSON array', () => {
    expect(parseIxLabelsText('[1, 2]').error).toMatch(/JSON array of strings/);
  });

  it('treats empty JSON array as no labels', () => {
    expect(parseIxLabelsText('[]')).toEqual({ labels: null, error: null });
  });
});

describe('parseIxLabelFilter / ixLabelsMatchFilter', () => {
  const labels = ['Pump.Fun: Create', 'Pump.Fun: Buy'];

  it('JSON array is ordered exact (case-insensitive)', () => {
    expect(parseIxLabelFilter('["Pump.Fun: Create","Pump.Fun: Buy"]')).toEqual({
      kind: 'json',
      needles: ['Pump.Fun: Create', 'Pump.Fun: Buy'],
    });
    expect(ixLabelsMatchFilter(labels, '["pump.fun: create","pump.fun: buy"]')).toBe(true);
    expect(ixLabelsMatchFilter(labels, '["Pump.Fun: Buy","Pump.Fun: Create"]')).toBe(false);
    expect(ixLabelsMatchFilter(labels, '["Pump.Fun: Create"]')).toBe(false);
  });

  it('accepts { instructions: [...] } JSON object', () => {
    expect(
      ixLabelsMatchFilter(labels, '{"instructions":["Pump.Fun: Create","Pump.Fun: Buy"]}'),
    ).toBe(true);
  });

  it('plain text is substring-any', () => {
    expect(ixLabelsMatchFilter(labels, 'Buy')).toBe(true);
    expect(ixLabelsMatchFilter(labels, 'Jito')).toBe(false);
    expect(ixLabelsMatchFilter(labels, 'Jito\nBuy')).toBe(true);
  });

  it('isIxLabelJsonFilter is true only for the JSON branch', () => {
    expect(isIxLabelJsonFilter('["a"]')).toBe(true);
    expect(isIxLabelJsonFilter('Buy')).toBe(false);
    expect(isIxLabelJsonFilter('[oops')).toBe(false);
  });
});
