"""The pricing kernel. One function books a trade; nothing else in this study may price a fill.

Laws (_!___strategy.md 6.1-6.3):
  * price = vsol^2 / K exactly; the state a transaction meets at time T is the last print at or before T.
  * The fill on BOTH legs is the last print that has landed by decision + LAG (115 ms verdict), never
    the first print after it, and never before the decision print itself.
  * Silence freezes price: an exit needs no print.
  * Cost: 125 bps per leg on the curve side, fixed 0.000225 SOL per leg, own impact through the exact
    curve arithmetic on the VIRTUAL reserve (a 0.2 SOL buy into vsol 50 moves it to 50.2).
Modes: entry / exit each 'lag' (verdict) or 'slot_end' (stress: the last print of the decision's slot,
pessimistic by about half a slot).
"""
import numpy as np

K = 3.219e16            # vsol (SOL) * vtok (raw units), exact on every print
FEE = 0.0125
FIX = 0.000225
LAG = 0.115
B_DEFAULT = 0.2


def net(v0, v1, B=B_DEFAULT):
    """Net SOL of buying B at reserve v0 and selling everything at reserve v1, exact curve arithmetic."""
    s = B / (1.0 + FEE)                      # SOL that reaches the curve
    tok = K / v0 - K / (v0 + s)              # raw tokens received
    out_curve = v1 - K / (K / v1 + tok)      # SOL the curve returns for the bag at reserve v1
    return out_curve * (1.0 - FEE) - B - 2.0 * FIX


def net_project(v0, v1, B=B_DEFAULT):
    """The project's pumpfun_impact closed form, kept only to assert the two agree."""
    return B * ((v1 / v0) ** 2 * (1 - B / v1) * (1 - FEE) / ((1 + B / v0) * (1 + FEE)) - 1) - 2 * FIX


def fill_idx(t, k, lag=LAG):
    """Index of the last print landed by t[k] + lag, never before k."""
    return max(k, int(np.searchsorted(t, t[k] + lag, side='right') - 1))


def time_idx(t, k, T):
    """Index of the last print at or before absolute time T, never before k (state at a clock deadline)."""
    return max(k, int(np.searchsorted(t, T, side='right') - 1))


def slot_end_idx(slot, k):
    n = len(slot); j = k
    while j + 1 < n and slot[j + 1] == slot[k]:
        j += 1
    return j


def _land(t, slot, j, mode, lag):
    return fill_idx(t, j, lag) if mode == 'lag' else slot_end_idx(slot, j)


def book(t, v, slot, k, spec, lag=LAG, entry='lag', exit='lag', tell=None, B=B_DEFAULT):
    """Book one trade decided on print k.

    spec: dict with any of
      cap       : seconds after the fill at which a clock exit is decided (required)
      tp        : take-profit, price percent above the fill
      trail     : trailing stop, price percent below the running peak (peak starts at the fill)
      arm       : the trail is live only once price is this percent above the fill
      stop      : a hard stop this percent below the FILL price. With 'arm' it is the UNARMED
                  protection and goes dead at the print that arms the trail; alone it is live for
                  the whole hold. A threshold derived on the RESERVE squares into this price basis
                  (reserve -25 % is price -43.75 %) - there is no other correct conversion.
      res_trail : True = the trail is measured on the RESERVE, not the price (regression use only)
      clock_lag : False = the clock exit fills at the deadline itself (regression use only)
      abort     : (T, g) - a NO-PROGRESS abort. At T seconds after the fill, read the state the
                  deadline meets; if price has not risen g percent above the fill, close there.
                  Unlike a stop it is conditioned on the ABSENCE of a move, so it fills on a quiet
                  tape rather than into the cascade a breach exit sells into; unlike a clock it
                  leaves a position that IS moving completely alone, so the winner stays uncapped.
                  Silence satisfies it: no print by T means no progress, and the position closes.
    tell  : optional bool array over the token's prints; the first True after entry closes the trade
    Returns (net_sol, entry_idx, exit_idx, reason).
    """
    n = len(t)
    ei = _land(t, slot, k, entry, lag)
    v0 = v[ei]; t0 = t[ei]
    cap = spec['cap']
    clock_lag = lag if (exit == 'lag' and spec.get('clock_lag', True)) else 0.0
    xi_cap = time_idx(t, ei, t0 + cap + clock_lag)
    best_j, reason = None, 'cap'
    if ei + 1 < n:
        ta = t[ei + 1:]; va = v[ei + 1:]
        inwin = ta <= t0 + cap
        if inwin.any():
            m = int(np.nonzero(inwin)[0][-1]) + 1
            va = va[:m]
            pr = (va / v0) ** 2
            cands = []
            if 'tp' in spec:
                h = pr >= 1.0 + spec['tp'] / 100.0
                if h.any():
                    cands.append((int(np.argmax(h)), 'tp'))
            if 'trail' in spec:
                if spec.get('res_trail'):
                    peak = np.maximum.accumulate(np.maximum(va, v0)); h = va <= peak * (1 - spec['trail'] / 100.0)
                else:
                    peak = np.maximum.accumulate(np.maximum(pr, 1.0)); h = pr <= peak * (1 - spec['trail'] / 100.0)
                if 'arm' in spec:
                    armed = pr >= 1.0 + spec['arm'] / 100.0
                    if armed.any():
                        h = h.copy(); h[:int(np.argmax(armed))] = False
                    else:
                        h = np.zeros_like(h)
                if h.any():
                    cands.append((int(np.argmax(h)), 'trail'))
            if 'stop' in spec:
                h = pr <= 1.0 - spec['stop'] / 100.0
                if 'arm' in spec:
                    armed = pr >= 1.0 + spec['arm'] / 100.0
                    if armed.any():
                        h = h.copy(); h[int(np.argmax(armed)):] = False
                if h.any():
                    cands.append((int(np.argmax(h)), 'stop'))
            if tell is not None:
                tl = tell[ei + 1:ei + 1 + m]
                if tl.any():
                    cands.append((int(np.argmax(tl)), 'tell'))
            if cands:
                j, reason = min(cands)
                best_j = ei + 1 + j
    if 'abort' in spec:
        a_t, a_g = spec['abort']
        if a_t < cap and (best_j is None or t[best_j] > t0 + a_t):
            dj = time_idx(t, ei, t0 + a_t)
            if (v[dj] / v0) ** 2 < 1.0 + a_g / 100.0:
                xi = time_idx(t, ei, t0 + a_t + clock_lag)
                return net(v0, v[xi], B), ei, xi, 'abort'
    xi = xi_cap if best_j is None else _land(t, slot, best_j, exit, lag)
    return net(v0, v[xi], B), ei, xi, reason


def daily(df, ycol='y', daycol='day', B=B_DEFAULT):
    """Summary row: total SOL, pct per trade, positive days, worst day, win rate."""
    a = df.groupby(daycol)[ycol].sum()
    return dict(n=len(df), sol=round(float(df[ycol].sum()), 2), pct=round(float(df[ycol].mean()) / B * 100, 2),
                pos='%d/%d' % (int((a > 0).sum()), a.size), worst=round(float(a.min()), 2),
                win=round(float((df[ycol] > 0).mean()) * 100, 1))
