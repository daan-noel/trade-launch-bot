import { describe, expect, it } from 'vitest';
import {
  formatIxLabelsText,
  isIxLabelJsonFilter,
  ixLabelsMatchFilter,
  parseIxLabelFilter,
  parseIxLabelsText,
} from './ixLabels';

describe('formatIxLabelsText', () => {
  it('pretty-prints a JSON array', () => {
    expect(formatIxLabelsText(['a', 'b'])).toBe('[\n  "a",\n  "b"\n]');
  });

  it('handles null/empty', () => {
    expect(formatIxLabelsText(null)).toBe('');
    expect(formatIxLabelsText([])).toBe('');
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
