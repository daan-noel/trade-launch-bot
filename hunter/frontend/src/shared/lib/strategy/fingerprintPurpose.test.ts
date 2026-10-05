import { describe, expect, it } from 'vitest';

import {
  copyFingerprintIds,
  fingerprintPurposes,
  purposeOf,
} from './fingerprintPurpose';

function rule(fingerprintId: string, copy: boolean) {
  return { fingerprint_id: fingerprintId, params: copy ? { copy: true } : {} };
}

describe('fingerprint purpose', () => {
  it('puts a fingerprint on Copy when every rule that uses it is a copy rule', () => {
    const purposes = fingerprintPurposes([
      rule('fp-copy', true),
      rule('fp-copy', true),
      rule('fp-metric', false),
      rule('fp-shared', true),
      rule('fp-shared', false),
    ]);
    expect(purposeOf('fp-copy', purposes)).toBe('copy');
    expect(purposeOf('fp-metric', purposes)).toBe('general');
    expect(purposeOf('fp-shared', purposes)).toBe('general');
    expect(purposeOf('fp-unused', purposes)).toBe('general');
    expect(copyFingerprintIds([rule('fp-copy', true), rule('fp-metric', false)])).toEqual(
      new Set(['fp-copy']),
    );
  });
});
