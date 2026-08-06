import type { ColumnDef } from 'components/table/types';
import type { TokenRecord } from 'types';
import { AgeCell } from 'components/table/AgeCell';
import { DateCell } from 'components/table/DateCell';
import { ageClass, formatAge, formatDecimalTrim, ratioClass } from 'utils/format';
import { AddressDisplay } from 'components/ui/AddressDisplay';
import { u64Num } from 'lib/u64Wire';
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
  const supply = u64Num(r.initial_supply_token);
  if (r.initial_buy_sol == null || supply == null || supply <= 0) {
    return null;
  }
  return r.initial_buy_sol / supply;
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

/**
 * Tokens-page columns that default to HIDDEN. The page lays out every column
 * (it ignores `APPENDED_HIDDEN_KEYS`), so its default visibility is specced here
 * directly — keeps the initial view to the fields watched most often.
 *
 * Shared by every `tokenColumns()` table (All Tokens, Trader Analysis, the
 * creation-stats drilldown, fingerprint matches) — they all want the same
 * layout, so this stays the single default rather than four `defaultCols` maps.
 * `cu_price` is deliberately NOT here: it reads as a launch-client tell next to
 * `cu_limit`, so it is up front on the token tables (the appended token-info set
 * still hides it on strategy tables, where the row is about the position).
 */
const TOKENS_HIDDEN_KEYS = new Set([
  'name',
  'creator',
  'create_tx',
  'created',
  'lifetime',
  'last_synced',
  'ath_timestamp',
  'ath_fep_ratio',
  'current_fep_ratio',
  'market_cap',
  'volume',
  'init_supply',
  'token_amount',
  'min_tokens_out',
]);

export function tokenColumns(): ColumnDef<TokenRecord>[] {
  const info = tokenInfoColumnMap();
  // Pull a shared token-info column and apply the Tokens-page width.
  const c = (key: string): ColumnDef<TokenRecord> => ({ ...info[key], width: TOKEN_COL_WIDTH[key] });

  const cols: ColumnDef<TokenRecord>[] = [
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
      key: 'mint_address',
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
      // Age in seconds — same units the cell reads ("2m" etc.). Enables numeric
      // filtering (>60, 5..10) client-side. Client-DERIVED (now − created), with
      // no matching server column, so on serverSide tables (e.g. TokensPage) this
      // operand is unsupported and won't round-trip — a no-op there, live client-side.
      filterNumber: (r) => ageSecondsOf(r),
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
        r.lifetime_secs != null ? (
          <span className={ageClass(r.lifetime_secs)}>
            {formatAge(r.lifetime_secs)}
          </span>
        ) : (
          '-'
        ),
      sortValue: (r) => r.lifetime_secs,
      searchValue: (r) =>
        r.lifetime_secs != null ? formatAge(r.lifetime_secs) : '',
      // Seconds — same units the cell reads. Backed by the real `lifetime_secs`
      // server column, so serverSide numeric filtering round-trips.
      filterNumber: (r) => r.lifetime_secs ?? null,
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

  return cols.map((col) =>
    TOKENS_HIDDEN_KEYS.has(col.key) ? { ...col, defaultVisible: false } : col,
  );
}
