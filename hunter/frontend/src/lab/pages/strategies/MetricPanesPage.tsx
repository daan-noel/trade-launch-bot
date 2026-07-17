import { useState } from 'react';
import { useSearchParams } from 'react-router-dom';

import { Input } from 'components/ui/Input';
import { Button } from 'components/ui/Button';
import { MetricPanes } from '@lab/components/strategy/MetricPanes';

/**
 * Token metric panes (lab app, FE4). Enter a mint (or arrive with `?mint=`) to see
 * every engine metric's trajectory over that token's life, with a selected rule's
 * thresholds overlaid. Registry-driven — new metrics appear here automatically.
 */
export function MetricPanesPage() {
  const [params, setParams] = useSearchParams();
  const mint = params.get('mint') ?? '';
  const [draft, setDraft] = useState(mint);

  return (
    <div className="flex flex-col gap-3 p-4">
      <h1 className="text-lg font-semibold text-text">Token metric panes</h1>
      <form
        className="flex items-center gap-2"
        onSubmit={(e) => {
          e.preventDefault();
          setParams(draft.trim() ? { mint: draft.trim() } : {});
        }}
      >
        <Input
          fieldSize="sm"
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          placeholder="token mint address"
          className="max-w-md font-mono"
        />
        <Button variant="primary" size="sm" type="submit">
          Load
        </Button>
      </form>
      {mint ? (
        <MetricPanes mint={mint} />
      ) : (
        <p className="text-[12px] text-text-dim/70">Enter a mint address to load its metric panes.</p>
      )}
    </div>
  );
}
