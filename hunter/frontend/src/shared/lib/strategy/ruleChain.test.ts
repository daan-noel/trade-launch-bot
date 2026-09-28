import { describe, expect, it } from 'vitest';
import { emptyRuleDoc, metricCond, newId, newLine, newStage, signalCond } from './ruleDoc';
import { buyChips, buyGlance, LOCK_CHOICES, lockWord, sellChips, sellGlance, signalChip } from './ruleChain';
import { flowSplit } from './ruleFlow';

describe('rule chain', () => {
  it('says what each Tries choice does', () => {
    expect(LOCK_CHOICES.map((c) => c.label)).toEqual(['Any Print', 'Once Per Coin', 'Once Per Slot']);
    expect(lockWord(null)).toBe('Any Print');
    expect(lockWord('token')).toBe('Once Per Coin');
    expect(lockWord('slot')).toBe('Once Per Slot');
    expect(LOCK_CHOICES[0].hint).toContain('every trade');
    expect(LOCK_CHOICES[1].hint).toContain('stops this coin');
    expect(LOCK_CHOICES[2].hint).toContain('per block');
    expect(LOCK_CHOICES[0].hint).not.toContain('Give up');
  });

  it('reads an empty rule as a first print and no sell', () => {
    const doc = emptyRuleDoc();
    expect(buyGlance(doc)).toBe('first print where the filters hold · Any Print');
    expect(sellGlance(doc)).toBe('no sell');
  });

  it('grows the sell chain by stage name', () => {
    const doc = emptyRuleDoc();
    doc.stop_loss = 30;
    doc.take_profit = 100;
    doc.enter.lock = 'token';
    doc.enter.event = [metricCond({ metric: 'm_state.age_sec' }, [[{ operator: '>=', value: 1 }]])];
    doc.stages = [newStage('early'), newStage('late'), newStage('ride')];
    expect(buyGlance(doc)).toContain('Once Per Coin');
    expect(sellGlance(doc)).toBe('TP 100 · SL 30 → early → late → ride');
  });

  it('keeps a signal off the sell chain', () => {
    const doc = emptyRuleDoc();
    doc.take_profit = 40;
    doc.always = [newLine()];
    doc.signals = [{ id: 's1', name: 'cashout', groups: [] }];
    expect(signalChip(doc)).toBe('cashout');
    expect(sellChips(doc).map((c) => c.key)).toEqual(['tpsl', 'always']);
    expect(sellGlance(doc)).toBe('TP 40 → Always');
    expect(signalChip(emptyRuleDoc(), true, true)).toBe('Signal');
  });

  it('keeps an open empty always on the editor chain and off the list', () => {
    const doc = emptyRuleDoc();
    expect(sellChips(doc)).toEqual([]);
    expect(sellChips(doc, true, 'always').map((c) => c.key)).toEqual(['always']);
  });

  it('draws TP/SL as rows in one column', () => {
    const doc = emptyRuleDoc();
    doc.stop_loss = 30;
    doc.take_profit = 100;
    doc.always = [newLine({ if: [metricCond({ metric: 'm_position.held_sec' }, [[{ operator: '>=', value: 90 }]])] })];
    const split = flowSplit(doc);
    expect(split.sell.map((b) => b.key)).toEqual(['tpsl', 'always']);
    expect(split.sell[0].pairs.map((pair) => `${pair[0].gate}|${pair[0].text}`)).toEqual(['TP|+100%', 'SL|-30%']);
    expect(split.sell[1].pairs[0].find((r) => r.gate === 'if')?.text).toBe('held_sec >= 90');
  });

  it('keeps again out of sell, and exclusive on the buy row', () => {
    const doc = emptyRuleDoc();
    doc.reentry = { cooldown_sec: 30, max_per_coin: 3 };
    doc.exclusive = true;
    doc.priority = 2;
    const split = flowSplit(doc);
    expect(split.sell.some((b) => b.key === 'again')).toBe(false);
    expect(split.again?.head).toBe('AGAIN');
    expect(split.again?.pairs[0][0].text).toBe('wait 30s, up to 3 buys');
    expect(split.entry.some((r) => r.text.includes('priority 2'))).toBe(true);
    expect(split.buy.some((b) => b.head === 'AGAIN')).toBe(false);
  });

  it('keeps an open empty On gate on the editor chain and off the list', () => {
    const doc = emptyRuleDoc();
    expect(buyChips(doc).map((c) => c.key)).toEqual(['lock']);
    expect(buyChips(doc, true, 'on').map((c) => `${c.group}:${c.key}`)).toEqual(['when:on', 'looking:lock']);
    expect(buyChips(doc, true, 'giveup').map((c) => `${c.group}:${c.key}`)).toEqual(['looking:lock', 'looking:giveup']);
  });

  it('draws only the cases this rule has', () => {
    const doc = emptyRuleDoc();
    doc.enter.lock = 'token';
    doc.enter.event = [metricCond({ metric: 'm_state.age_sec' }, [[{ operator: '>=', value: 1 }]])];
    doc.enter.filters = [metricCond({ metric: 'm_state.liquidity_sol' }, [[{ operator: '>=', value: 80 }]])];
    const start = newStage('start');
    start.ends = { basis: 'age_sec', secs: 20 };
    start.then = 'armed';
    start.on = [newLine({ if: [metricCond({ metric: 'm_flow.buy_sol', tag: '!volume', span: '10s' }, [[{ operator: '>=', value: 2 }]])] })];
    doc.stages = [start, newStage('armed')];
    const split = flowSplit(doc, 'crew');
    expect(split.sell.map((b) => b.head)).toEqual(['start', 'armed']);
    expect(split.buy.map((b) => b.head)).toEqual(['On', 'Only If']);
    const entry = split.entry.map((r) => `${r.head}|${r.gate}|${r.text}`).join('\n');
    expect(entry).toContain('WATCH||crew');
    expect(entry).toContain('BUY|Once Per Coin|');
    expect(split.buy[0].pairs[0][0].text).toContain('age_sec >= 1');
    expect(split.buy[1].pairs[0][0].text).toContain('liquidity_sol >= 80');
    expect(entry).not.toContain('keep watching');
    expect(entry).not.toContain('stop watching');
    expect(entry).not.toContain('a tick never buys');
    expect(entry).not.toContain('do not buy');
    expect(split.buy.some((b) => b.head === 'Give Up')).toBe(false);
    const pair = split.sell[0].pairs[0];
    expect(pair.map((r) => `${r.gate}|${r.text}`).join('\n')).toContain('if|buy_sol @!volume [10s] >= 2');
    expect(pair.find((r) => r.gate === 'if')?.brace).toBe('open');
    const then = pair.find((r) => r.gate === 'then');
    expect(then?.brace).toBe('close');
    expect(then?.acts).toEqual([{ gate: 'sell', text: 'all' }]);
    expect(pair.some((r) => r.gate === 'sell')).toBe(false);
    const deadline = split.sell[0].pairs[1][0];
    expect(deadline.gate).toBe('deadline');
    expect(deadline.text).toBe('coin age 20s');
    expect(deadline.acts).toEqual([{ gate: 'go', text: 'armed' }]);
    expect(split.sell[1].pairs[0].map((r) => r.text)).toEqual(['no line']);
  });

  it('marks a signal used inside a stage so the view can color it', () => {
    const doc = emptyRuleDoc();
    const ride = newStage('ride');
    ride.on = [newLine({ if: [signalCond('cashout'), metricCond({ metric: 'm_state.age_sec' }, [[{ operator: '<', value: 20 }]])] })];
    doc.signals = [{ id: newId(), name: 'cashout', groups: [[metricCond({ metric: 'm_holdings.profit_sol' })]] }];
    doc.stages = [ride];
    const split = flowSplit(doc);
    const row = split.sell.find((b) => b.head === 'ride')?.pairs[0].find((r) => r.signal);
    expect(row?.signal).toBe('cashout');
    expect(row?.text).toBe('cashout');
    expect(row?.metric).toBeUndefined();
    expect(split.signals.map((b) => b.head)).toEqual(['cashout']);
    expect(split.sell.some((b) => b.head === 'cashout')).toBe(false);
  });

  it('draws a signal named from a buy gate on that gate', () => {
    const doc = emptyRuleDoc();
    doc.signals = [{ id: newId(), name: 'door', groups: [[metricCond({ metric: 'm_state.liquidity_sol' }, [[{ operator: '>=', value: 14 }]])]] }];
    doc.enter.event = [signalCond('door')];
    doc.enter.filters = [signalCond('door', true)];
    const split = flowSplit(doc);
    expect(split.buy[0].pairs[0][0].signal).toBe('door');
    expect(split.buy[1].pairs[0][0].text).toBe('not door');
    expect(split.signals[0].head).toBe('door');
  });

  it('skips a parked On condition and a signal condition still counts', () => {
    const doc = emptyRuleDoc();
    const parked = metricCond({ metric: 'm_state.age_sec' }, [[{ operator: '>=', value: 1 }]], true);
    doc.enter.event = [parked, signalCond('cashout')];
    expect(buyGlance(doc)).toBe('cashout · Any Print');
  });
});
