import { describe, expect, it } from 'vitest';
import { formatIxLabelsText, parseIxLabelsText } from './ixLabels';

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
