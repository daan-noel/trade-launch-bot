import { describe, it, expect } from 'vitest';
import { applyTableRequest, type ColResolver } from './tableEval';
import type { TableRequestBody } from './tableRequest';
import fixtures from './tableEval.fixtures.json';

// Rust<->TS evaluator conformance. The SAME fixture file drives the Rust
// `trading_core::api::table_eval` `conformance` test (via include_str!) and this
// one, so the two in-memory evaluators (Simulated in Rust, Wallet in TS) can't
// drift on op/sort/search/tiebreak/paging semantics. See the fixture's `_comment`
// for the shared grammar.

interface Row {
  mint: string;
  symbol: string;
  pnl_percent: number | null;
  trade_count: number | null;
  exit_reason: string | null;
}

/** The fixture grammar — mirrors the Rust test's `resolve`. */
const resolve: ColResolver<Row> = (key) => {
  switch (key) {
    case 'mint':
      return { kind: 'text', text: (r) => r.mint ?? null, sort: (r) => r.mint ?? null };
    case 'symbol':
      return { kind: 'text', text: (r) => r.symbol ?? null, sort: (r) => r.symbol ?? null };
    case 'reason':
      return { kind: 'text', text: (r) => r.exit_reason ?? null, sort: (r) => r.exit_reason ?? null };
    case 'pnl':
      return { kind: 'number', num: (r) => r.pnl_percent ?? null, sort: (r) => r.pnl_percent ?? null };
    case 'trades':
      return { kind: 'number', num: (r) => r.trade_count ?? null, sort: (r) => r.trade_count ?? null };
    default:
      return null;
  }
};

/** Fill a partial fixture request out to a full body, applying the same field
 *  defaults the Rust `TableRequest` serde defaults do (pageSize 200). */
function body(req: Record<string, unknown>): TableRequestBody {
  const pg = (req.pagination ?? {}) as { page?: number; pageSize?: number };
  return {
    pagination: { page: pg.page ?? 1, pageSize: pg.pageSize ?? 200 },
    sorting: (req.sorting ?? []) as TableRequestBody['sorting'],
    search: (req.search ?? '') as string,
    filters: (req.filters ?? {}) as TableRequestBody['filters'],
  };
}

const rows = fixtures.rows as Row[];

describe('tableEval conformance (shared Rust<->TS fixtures)', () => {
  for (const c of fixtures.cases) {
    it(c.name, () => {
      const { items } = applyTableRequest(rows, body(c.request as Record<string, unknown>), resolve);
      expect(items.map((r) => r.mint)).toEqual(c.expected);
    });
  }
});
