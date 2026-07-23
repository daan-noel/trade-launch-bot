import { Modal } from 'components/ui/Modal';
import { apiErrorMessage, useGetTokenDetailQuery } from 'store/apiSlice';
import { truncate } from 'utils/format';
import { TokenDetailPanel } from './TokenDetailPanel';
import { LazyTokenTradeChart } from './LazyTokenTradeChart';

interface TokenDetailModalProps {
  /** Mint to load — the modal is mounted only while a row is selected, so this
   *  is never empty (the caller conditionally renders on a non-null mint). */
  mint: string;
  /** Row's already-known symbol, shown in the title while the detail fetch is
   *  in flight (falls back to a truncated mint). */
  symbol?: string;
  onClose: () => void;
  /** Column-visibility persistence key for the embedded trade chart's table
   *  bits. Defaults to one shared key — pass a caller-specific one if several
   *  call sites need independent column prefs. */
  tableId?: string;
}

/**
 * Read-only token detail as a modal overlay — the stats/chart panel other
 * pages (Tokens, Sync) render inline below their table, packaged for call
 * sites where an inline panel doesn't fit (a drill-down table nested inside an
 * already-scrolling dashboard section). Fetches via the same
 * `useGetTokenDetailQuery` those pages use, so the content is identical.
 */
export function TokenDetailModal({ mint, symbol, onClose, tableId }: TokenDetailModalProps) {
  const { data: detail, isFetching, error } = useGetTokenDetailQuery(mint, { skip: !mint });
  const heading = symbol || truncate(mint, 8);

  return (
    <Modal title={`${heading} — Token detail`} open onClose={onClose} size="xxl">
      <div className="flex flex-col gap-3.5">
        <TokenDetailPanel
          detail={detail ?? null}
          loading={isFetching}
          error={apiErrorMessage(error, 'Failed to load detail')}
        />
        <LazyTokenTradeChart
          key={mint}
          tableId={tableId ?? 'token_detail_modal_trades'}
          detail={detail ?? null}
        />
      </div>
    </Modal>
  );
}
