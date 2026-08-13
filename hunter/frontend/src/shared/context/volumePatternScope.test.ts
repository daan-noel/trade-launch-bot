import { describe, expect, it } from 'vitest';

import { resolveFlowPatternKeys } from './VolumePatternDraftContext';

/**
 * The scope decides which set a view CLASSIFIES with — a different question from
 * whether patterns may be edited there. Collapsing the two costs one of them: a
 * position modal that refuses to redraw under a draft (so an engine decision is
 * never silently re-classified) must still let a pattern be staged from its trades
 * table, which is exactly how a misclassified bot tx gets found.
 */

const SAVED = new Set(['["saved"]']);
const DRAFT = new Set(['["saved"]', '["staged"]']);

describe('resolveFlowPatternKeys', () => {
  it('applies an open draft ambiently in a free scope', () => {
    const r = resolveFlowPatternKeys({
      mode: 'free',
      preview: false,
      draftKeys: DRAFT,
      propKeys: SAVED,
    });
    expect(r.keys).toBe(DRAFT);
    expect(r.draftActive).toBe(true);
    expect(r.draftIgnored).toBe(false);
  });

  it('a decision scope keeps the SAVED set until preview is asked for', () => {
    const rest = resolveFlowPatternKeys({
      mode: 'decision',
      preview: false,
      draftKeys: DRAFT,
      propKeys: SAVED,
    });
    expect(rest.keys).toBe(SAVED);
    expect(rest.draftActive).toBe(false);
    // Staged-but-not-applied must be visible, or the reader hand-sums a split that
    // disagrees with the exit sitting next to it.
    expect(rest.draftIgnored).toBe(true);
    // ...and editing is never blocked here; the switch is offered instead.
    expect(rest.canPreviewDraft).toBe(true);
    expect(rest.locked).toBe(false);

    const previewing = resolveFlowPatternKeys({
      mode: 'decision',
      preview: true,
      draftKeys: DRAFT,
      propKeys: SAVED,
    });
    expect(previewing.keys).toBe(DRAFT);
    expect(previewing.draftActive).toBe(true);
    expect(previewing.draftIgnored).toBe(false);
  });

  it('a locked scope neither applies nor offers the draft, even asked to preview', () => {
    const r = resolveFlowPatternKeys({
      mode: 'locked',
      preview: true,
      draftKeys: DRAFT,
      propKeys: SAVED,
    });
    expect(r.keys).toBe(SAVED);
    expect(r.draftActive).toBe(false);
    expect(r.canPreviewDraft).toBe(false);
    expect(r.locked).toBe(true);
  });

  it('with no draft open every scope reads its own saved set and flags nothing', () => {
    for (const mode of ['free', 'decision', 'locked'] as const) {
      const r = resolveFlowPatternKeys({
        mode,
        preview: true,
        draftKeys: null,
        propKeys: SAVED,
      });
      expect(r.keys, mode).toBe(SAVED);
      expect(r.draftActive, mode).toBe(false);
      expect(r.draftIgnored, mode).toBe(false);
      expect(r.canPreviewDraft, mode).toBe(false);
    }
  });

  it('falls back to null when the host has no saved keys either', () => {
    const r = resolveFlowPatternKeys({
      mode: 'decision',
      preview: false,
      draftKeys: null,
      propKeys: undefined,
    });
    expect(r.keys).toBeNull();
  });
});
