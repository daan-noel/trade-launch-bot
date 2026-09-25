// The searched fingerprint's tags, one sentence each: what a tagged read splits on.
// Rule search and family search both read the fingerprint's own tags, so both show
// this under the fingerprint picker.

import { LabelTip } from 'components/strategy/LabelTip';
import type { HelpTip } from 'lib/strategy/strategyHelp';
import { tagSentence, tagsFromJson } from 'lib/strategy/tagsDoc';

const TIP: HelpTip = {
  title: 'Tags',
  body: 'The fingerprint\'s tags split every coin\'s trades: `@volume` reads the tagged trades, `@!volume` the rest. The search tries each tagged read per tag; with no tags, tagged reads are left out. Edit tags on the fingerprint.',
};

export function TagsNote({ tags }: { tags: Record<string, unknown> }) {
  const defs = tagsFromJson(tags);
  return (
    <div className="mt-2 text-[11px] text-text-dim">
      <LabelTip tip={TIP}>
        <span className="text-[9px] font-bold uppercase tracking-wider text-text-dim/80">Tags</span>
      </LabelTip>
      {defs.length ? (
        <ul className="mt-0.5 space-y-0.5 font-mono text-text-mid">
          {defs.map((t) => (
            <li key={t.name}>{tagSentence(t)}</li>
          ))}
        </ul>
      ) : (
        <p className="mt-0.5">None: tagged reads (e.g. m_flow.buy_sol @volume) are left out of the search.</p>
      )}
    </div>
  );
}
