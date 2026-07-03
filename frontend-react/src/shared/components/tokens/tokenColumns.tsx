import type { ColumnDef } from 'components/table/types';
import type { TokenRecord } from 'types';
import { AgeCell } from 'components/table/AgeCell';
import { DateCell } from 'components/table/DateCell';
import { ageClass, formatAge, formatDecimalTrim, ratioClass } from 'utils/format';
import { AddressDisplay } from 'components/ui/AddressDisplay';
import { tokenInfoColumnMap } from './sharedTokenColumns';

/**
 * Token creation as epoch-ms. Prefers the field the RTK transform pre-parsed
 * once (`created_at_ms`), falling back to parsing the ISO string for any row
 * that predates it. The age itself is rendered live by {@link AgeCell}, which
 * subscribes to the shared clock and ticks on its own between polls.
 */
function createdMsOf(r: TokenRecord): number {
  return r.created_at_ms ?? Date.parse(r.created_at);
}

/** Age in whole seconds for the sort/search fallbacks (dead in server mode). */
function ageSecondsOf(r: TokenRecord): number {
  return Math.max(0, Math.floor((Date.now() - createdMsOf(r)) / 1000));
}

function fep(r: TokenRecord): number | null {
  if (r.initial_buy_sol == null || r.initial_supply_token == null || r.initial_supply_token <= 0) {
    return null;
  }
  return r.initial_buy_sol / r.initial_supply_token;
}

// Per-column pixel widths for the Tokens-page layout. The shared token-info
// columns (from `tokenInfoColumnMap`) carry no width — the Tokens page owns
// them here; the strategy tables render the same columns auto-sized.
const TOKEN_COL_WIDTH: Record<string, string> = {
  creator: '165px',
  create_tx: '165px',
  last_trade: '110px',
  last_synced: '96px',
  trade_count: '66px',
  ath_price: '88px',
  ath_timestamp: '110px',
  current_price: '88px',
  market_cap: '84px',
  volume: '78px',
  first_slot_buy: '84px',
  first_slot_sell: '84px',
  initial_buy: '78px',
  init_supply: '90px',
  token_amount: '90px',
  max_cost_lamports: '100px',
  spendable_lamports_in: '100px',
  min_tokens_out: '90px',
  cu_limit: '72px',
  cu_price: '72px',
  ix_count: '54px',
  ix_labels: '180px',
  migrated: '66px',
  dead: '66px',
  mayhem_mode: '66px',
  cashback: '72px',
};

export function tokenColumns(): ColumnDef<TokenRecord>[] {
  const info = tokenInfoColumnMap();
  // Pull a shared token-info column and apply the Tokens-page width.
  const c = (key: string): ColumnDef<TokenRecord> => ({ ...info[key], width: TOKEN_COL_WIDTH[key] });

  return [
    // identity — symbol/name/mint are Tokens-page-specific (own rendering);
    // creator/create_tx come from the shared SSOT.
    {
      key: 'symbol',
      label: 'Symbol',
      group: 'identity',
      width: '90px',
      sortable: true,
      render: (r) => r.symbol,
      sortValue: (r) => r.symbol,
      searchValue: (r) => `${r.symbol} ${r.name} ${r.mint_address}`,
    },
    {
      key: 'name',
      label: 'Name',
      group: 'identity',
      width: '120px',
      sortable: true,
      render: (r) => r.name,
      sortValue: (r) => r.name,
      searchValue: (r) => r.name,
    },
    {
      key: 'mint',
      label: 'Mint',
      group: 'identity',
      width: '165px',
      sortable: true,
      render: (r) => (
        <AddressDisplay address={r.mint_address} kind="token" stopPropagation />
      ),
      sortValue: (r) => r.mint_address,
      searchValue: (r) => r.mint_address,
    },
    c('creator'),
    c('create_tx'),
    // activity
    {
      key: 'token_age',
      label: 'Token Age',
      group: 'activity',
      width: '72px',
      sortable: true,
      render: (r) => <AgeCell createdMs={createdMsOf(r)} />,
      sortValue: (r) => ageSecondsOf(r),
      searchValue: (r) => formatAge(ageSecondsOf(r)),
    },
    {
      key: 'created',
      label: 'Created',
      group: 'activity',
      width: '110px',
      sortable: true,
      render: (r) => <DateCell iso={r.created_at} />,
      sortValue: (r) => r.created_at,
      searchValue: (r) => r.created_at,
    },
    c('last_trade'),
    {
      key: 'lifetime',
      label: 'Lifetime',
      group: 'activity',
      width: '72px',
      sortable: true,
      render: (r) =>
        r.active_lifetime_secs != null ? (
          <span className={ageClass(r.active_lifetime_secs)}>
            {formatAge(r.active_lifetime_secs)}
          </span>
        ) : (
          '-'
        ),
      sortValue: (r) => r.active_lifetime_secs,
      searchValue: (r) =>
        r.active_lifetime_secs != null ? formatAge(r.active_lifetime_secs) : '',
    },
    c('last_synced'),
    c('trade_count'),
    // price
    c('ath_price'),
    c('ath_timestamp'),
    {
      key: 'ath_fep_ratio',
      label: 'ATH/FEP',
      group: 'price',
      width: '88px',
      sortable: true,
      render: (r) => {
        const entry = fep(r);
        const ratio =
          entry && r.ath_price && entry !== 0 ? r.ath_price / entry : null;
        return ratio != null ? (
          <span className={ratioClass(ratio)}>{formatDecimalTrim(ratio, 2)}x</span>
        ) : (
          '-'
        );
      },
      sortValue: (r) => {
        const entry = fep(r);
        return entry && r.ath_price && entry !== 0 ? r.ath_price / entry : null;
      },
      searchValue: () => '',
      filterValue: (r) => {
        const entry = fep(r);
        const ratio = entry && r.ath_price && entry !== 0 ? r.ath_price / entry : null;
        return ratio != null ? `${formatDecimalTrim(ratio, 2)}x` : '';
      },
      filterNumber: (r) => {
        const entry = fep(r);
        return entry && r.ath_price && entry !== 0 ? r.ath_price / entry : null;
      },
    },
    c('current_price'),
    {
      key: 'current_fep_ratio',
      label: 'Cur/FEP',
      group: 'price',
      width: '76px',
      sortable: true,
      render: (r) => {
        const entry = fep(r);
        const ratio =
          entry && r.current_price && entry !== 0 ? r.current_price / entry : null;
        return ratio != null ? (
          <span className={ratioClass(ratio)}>{formatDecimalTrim(ratio, 2)}x</span>
        ) : (
          '-'
        );
      },
      sortValue: (r) => {
        const entry = fep(r);
        return entry && r.current_price && entry !== 0 ? r.current_price / entry : null;
      },
      searchValue: () => '',
      filterValue: (r) => {
        const entry = fep(r);
        const ratio = entry && r.current_price && entry !== 0 ? r.current_price / entry : null;
        return ratio != null ? `${formatDecimalTrim(ratio, 2)}x` : '';
      },
      filterNumber: (r) => {
        const entry = fep(r);
        return entry && r.current_price && entry !== 0 ? r.current_price / entry : null;
      },
    },
    // market
    c('market_cap'),
    c('volume'),
    c('first_slot_buy'),
    c('first_slot_sell'),
    // initial
    c('initial_buy'),
    c('init_supply'),
    // max_or_spendable
    c('token_amount'),
    c('max_cost_lamports'),
    c('spendable_lamports_in'),
    c('min_tokens_out'),
    // compute
    c('cu_limit'),
    c('cu_price'),
    // ix
    c('ix_count'),
    c('ix_labels'),
    // flags
    c('migrated'),
    c('dead'),
    c('mayhem_mode'),
    c('cashback'),
  ];
}
