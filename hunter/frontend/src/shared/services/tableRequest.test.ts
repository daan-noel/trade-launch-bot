import { describe, expect, it } from 'vitest';

import { setPriceUnitSnapshot } from 'lib/priceUnitSnapshot';
import { toTableRequest } from './tableRequest';

describe('toTableRequest PriceUnit amountCols', () => {
  it('converts displayed USD operands to SOL storage', () => {
    setPriceUnitSnapshot({ unit: 'USD', usdRate: 150 });
    const body = toTableRequest(
      {
        page: 1,
        pageSize: 50,
        sortKeys: [],
        search: '',
        colFilters: { market_cap: '>300', volume: '150..450' },
      },
      new Set(['market_cap', 'volume']),
      { amountCols: new Map([['market_cap', 'sol'], ['volume', 'sol']]) },
    );
    expect(body.filters.market_cap).toEqual({ op: 'gt', val: 2 });
    expect(body.filters.volume).toEqual({ op: 'between', min: 1, max: 3 });
  });

  it('leaves structuredFilters in storage units (dust gate)', () => {
    setPriceUnitSnapshot({ unit: 'SOL', usdRate: 150 });
    const body = toTableRequest(
      {
        page: 1,
        pageSize: 50,
        sortKeys: [],
        search: '',
        colFilters: {},
        structuredFilters: { value_usd: { op: 'gte', val: 1 } },
      },
      new Set(['value_usd']),
      { amountCols: new Map([['value_usd', 'usd']]) },
    );
    expect(body.filters.value_usd).toEqual({ op: 'gte', val: 1 });
  });
});
