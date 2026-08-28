"""Honest-exit walk for the ix-template gate.

Entry and exit both fill at the last print with ts <= fire + lag_ms (fallback: the
firing print / last known print). First-gap is a 2-slot (~800 ms) buy silence after
the last buy. Armed trail arms at +arm_pct from entry fill, then exits at trail_pct
off the in-hold peak; if it never arms, first-gap takes over.

Usage: DATABASE_URL in env. Writes a summary to stdout.
"""
from __future__ import annotations

import os
import sys
from collections import defaultdict

import numpy as np
import pandas as pd
import psycopg2

B = 0.10e9
FEE_BUY = 1.0125
FEE_SELL = 0.9875
SLOT_MS = 400
GAP_SLOTS = 2
HOLD_CAP_S = 300
CLOCK_S = 20
ARM_PCT = 10.0
TRAIL_PCT = 18.0


def net_ret(px_e, v_e, px_x, v_x):
    if px_e is None or px_x is None or not (v_e > 0) or not (v_x > 0):
        return None
    return (px_x * (1.0 - B / max(v_x, 1.0)) * FEE_SELL) / (
        px_e * (1.0 + B / max(v_e, 1.0)) * FEE_BUY
    ) - 1.0


def fill_idx(times, fire_ts, lag_ms, lo, n):
    """Last index i in [lo, n) with times[i] <= fire_ts + lag. Fallback lo."""
    deadline = fire_ts + np.timedelta64(int(lag_ms), "ms")
    i = lo
    j = lo
    while j + 1 < n and times[j + 1] <= deadline:
        j += 1
    if times[j] <= deadline:
        return j
    return i


def walk_harvest(times, slots, px, vsol, is_buy, eidx, ets, epx, ev, last_buy_ts, last_buy_i, lag_ms, hold, cap_at):
    """Leave on dump/death after the first buy-gap; stay on a second buy wave.

    After fill, wait for 2-slot buy silence. Then WATCH: a sell or a second
    silence is dump/death (exit). Two post-gap buys confirm a wave and HOLD
    with `hold` = 'clock' (20 s from fill) or 'trail' (arm 10 / trail 18;
    unarmed falls back to the next buy-gap).
    """
    n = len(times)
    gap_delay = np.timedelta64(GAP_SLOTS * SLOT_MS, "ms")
    clock_at = ets + np.timedelta64(CLOCK_S, "s")
    arm_lvl = epx * (1.0 + ARM_PCT / 100.0)
    trail_keep = 1.0 - TRAIL_PCT / 100.0
    peak = epx
    armed = False
    state = "run"
    watch_buys = 0
    watch_until = None

    def close_at(fire_ts, lo, reason):
        xidx = fill_idx(times, fire_ts, lag_ms, lo, n)
        r = net_ret(epx, ev, px[xidx], vsol[xidx])
        if r is None:
            return None
        hold_s = (times[xidx] - ets) / np.timedelta64(1, "s")
        return (r, float(hold_s), reason, times[xidx])

    for k in range(eidx + 1, n):
        t = times[k]
        if t >= cap_at:
            return close_at(cap_at, k, "cap")

        if state == "run":
            gap_at = last_buy_ts + gap_delay
            if t > gap_at:
                state = "watch"
                watch_until = gap_at + gap_delay
                watch_buys = 0
            else:
                if is_buy[k]:
                    last_buy_ts = t
                    last_buy_i = k
                continue

        if state == "watch":
            if watch_until is not None and t > watch_until and watch_buys < 2:
                return close_at(watch_until, last_buy_i, "death")
            if is_buy[k]:
                watch_buys += 1
                last_buy_ts = t
                last_buy_i = k
                if watch_buys >= 2:
                    state = "hold"
                else:
                    watch_until = t + gap_delay
                continue
            return close_at(t, k, "dump")

        # hold
        if hold == "clock":
            if t > clock_at:
                return close_at(clock_at, max(eidx, last_buy_i), "clock")
        else:
            p = px[k]
            if p > 0:
                if not armed:
                    if p >= arm_lvl:
                        armed = True
                        peak = p
                else:
                    if p > peak:
                        peak = p
                    elif p <= peak * trail_keep:
                        return close_at(t, k, "trail")
            if not armed:
                gap_at = last_buy_ts + gap_delay
                if t > gap_at:
                    return close_at(gap_at, last_buy_i, "gap")
        if is_buy[k]:
            last_buy_ts = t
            last_buy_i = k

    if state != "hold":
        reason = "death" if state == "watch" else "gap"
        gap_at = last_buy_ts + gap_delay
        fire = gap_at if gap_at <= cap_at else cap_at
        if state == "watch" and watch_until is not None:
            fire = watch_until if watch_until <= cap_at else cap_at
        return close_at(fire, last_buy_i, reason)
    if hold == "clock":
        fire = clock_at if clock_at <= cap_at else cap_at
        return close_at(fire, last_buy_i, "clock")
    if armed:
        return close_at(cap_at, last_buy_i, "cap")
    gap_at = last_buy_ts + gap_delay
    fire = gap_at if gap_at <= cap_at else cap_at
    return close_at(fire, last_buy_i, "gap")


def _close_at(times, px, vsol, epx, ev, ets, lag_ms, n, fire_ts, lo, reason):
    xidx = fill_idx(times, fire_ts, lag_ms, lo, n)
    r = net_ret(epx, ev, px[xidx], vsol[xidx])
    if r is None:
        return None
    hold_s = (times[xidx] - ets) / np.timedelta64(1, "s")
    return (r, float(hold_s), reason, times[xidx])


def walk_trail_hold(times, px, vsol, eidx, ets, epx, ev, lag_ms, cap_at):
    """Armed trail; unarmed holds to cap. No first-gap fallback."""
    n = len(times)
    arm_lvl = epx * (1.0 + ARM_PCT / 100.0)
    trail_keep = 1.0 - TRAIL_PCT / 100.0
    peak = epx
    armed = False
    for k in range(eidx + 1, n):
        t = times[k]
        if t >= cap_at:
            return _close_at(times, px, vsol, epx, ev, ets, lag_ms, n, cap_at, k, "cap")
        p = px[k]
        if p > 0:
            if not armed:
                if p >= arm_lvl:
                    armed = True
                    peak = p
            else:
                if p > peak:
                    peak = p
                elif p <= peak * trail_keep:
                    return _close_at(
                        times, px, vsol, epx, ev, ets, lag_ms, n, t, k, "trail"
                    )
    return _close_at(times, px, vsol, epx, ev, ets, lag_ms, n, cap_at, eidx, "cap")


def walk_death(times, px, vsol, is_buy, eidx, ets, epx, ev, last_buy_ts, last_buy_i, lag_ms, cap_at, death_s):
    """Exit on death_s seconds of buy silence. Stay while buys keep printing."""
    n = len(times)
    death_delay = np.timedelta64(int(death_s), "s")
    for k in range(eidx + 1, n):
        t = times[k]
        if t >= cap_at:
            return _close_at(times, px, vsol, epx, ev, ets, lag_ms, n, cap_at, k, "cap")
        death_at = last_buy_ts + death_delay
        if t > death_at:
            fire = death_at if death_at <= cap_at else cap_at
            return _close_at(
                times, px, vsol, epx, ev, ets, lag_ms, n, fire, last_buy_i, "death"
            )
        if is_buy[k]:
            last_buy_ts = t
            last_buy_i = k
    death_at = last_buy_ts + death_delay
    fire = death_at if death_at <= cap_at else cap_at
    reason = "death" if death_at <= cap_at else "cap"
    return _close_at(
        times, px, vsol, epx, ev, ets, lag_ms, n, fire, last_buy_i, reason
    )


def walk_arm_death(times, px, vsol, is_buy, eidx, ets, epx, ev, last_buy_ts, last_buy_i, lag_ms, cap_at, death_s):
    """Armed trail; unarmed dies on death_s buy silence. Pause after arm is not death."""
    n = len(times)
    death_delay = np.timedelta64(int(death_s), "s")
    arm_lvl = epx * (1.0 + ARM_PCT / 100.0)
    trail_keep = 1.0 - TRAIL_PCT / 100.0
    peak = epx
    armed = False
    for k in range(eidx + 1, n):
        t = times[k]
        if t >= cap_at:
            return _close_at(times, px, vsol, epx, ev, ets, lag_ms, n, cap_at, k, "cap")
        p = px[k]
        if p > 0:
            if not armed:
                if p >= arm_lvl:
                    armed = True
                    peak = p
            else:
                if p > peak:
                    peak = p
                elif p <= peak * trail_keep:
                    return _close_at(
                        times, px, vsol, epx, ev, ets, lag_ms, n, t, k, "trail"
                    )
        if not armed:
            death_at = last_buy_ts + death_delay
            if t > death_at:
                fire = death_at if death_at <= cap_at else cap_at
                return _close_at(
                    times, px, vsol, epx, ev, ets, lag_ms, n, fire, last_buy_i, "death"
                )
        if is_buy[k]:
            last_buy_ts = t
            last_buy_i = k
    if armed:
        return _close_at(times, px, vsol, epx, ev, ets, lag_ms, n, cap_at, last_buy_i, "cap")
    death_at = last_buy_ts + death_delay
    fire = death_at if death_at <= cap_at else cap_at
    reason = "death" if death_at <= cap_at else "cap"
    return _close_at(
        times, px, vsol, epx, ev, ets, lag_ms, n, fire, last_buy_i, reason
    )


def walk_one(times, slots, px, vsol, is_buy, trig, lag_ms, mode, clock_s=None, death_s=None):
    """trig is (idx, ts, slot). Returns (net, hold_s, reason, exit_ts) or None."""
    n = len(times)
    ti, tts, tslot = trig
    if ti < 0 or ti >= n or px[ti] <= 0 or vsol[ti] <= 0:
        return None
    eidx = fill_idx(times, tts, lag_ms, ti, n)
    epx, ev = px[eidx], vsol[eidx]
    if epx <= 0 or ev <= 0:
        return None
    ets = times[eidx]
    cap_at = tts + np.timedelta64(HOLD_CAP_S, "s")
    clock_hold = CLOCK_S if clock_s is None else int(clock_s)
    clock_at = tts + np.timedelta64(clock_hold, "s")
    gap_delay = np.timedelta64(GAP_SLOTS * SLOT_MS, "ms")
    if mode in ("harvest_clock", "harvest_trail"):
        last_buy_ts = tts
        last_buy_i = ti
        if is_buy[eidx]:
            last_buy_ts = times[eidx]
            last_buy_i = eidx
        hold = "clock" if mode == "harvest_clock" else "trail"
        return walk_harvest(
            times, slots, px, vsol, is_buy,
            eidx, ets, epx, ev, last_buy_ts, last_buy_i,
            lag_ms, hold, cap_at,
        )
    last_buy_ts = tts
    last_buy_i = ti
    if is_buy[eidx]:
        last_buy_ts = times[eidx]
        last_buy_i = eidx
    if mode == "trail_hold":
        return walk_trail_hold(times, px, vsol, eidx, ets, epx, ev, lag_ms, cap_at)
    if mode == "death":
        ds = 8 if death_s is None else int(death_s)
        return walk_death(
            times, px, vsol, is_buy,
            eidx, ets, epx, ev, last_buy_ts, last_buy_i,
            lag_ms, cap_at, ds,
        )
    if mode == "arm_death":
        ds = 8 if death_s is None else int(death_s)
        return walk_arm_death(
            times, px, vsol, is_buy,
            eidx, ets, epx, ev, last_buy_ts, last_buy_i,
            lag_ms, cap_at, ds,
        )

    last_buy_ts = tts
    last_buy_i = ti
    peak = epx
    armed = False
    arm_lvl = epx * (1.0 + ARM_PCT / 100.0)
    trail_keep = 1.0 - TRAIL_PCT / 100.0

    def close_at(fire_ts, lo, reason):
        xidx = fill_idx(times, fire_ts, lag_ms, lo, n)
        r = net_ret(epx, ev, px[xidx], vsol[xidx])
        if r is None:
            return None
        hold = (times[xidx] - ets) / np.timedelta64(1, "s")
        return (r, float(hold), reason, times[xidx])

    for k in range(ti + 1, n):
        gap_at = last_buy_ts + gap_delay
        fire_gap = gap_at if gap_at <= cap_at else cap_at
        gap_reason = "gap" if gap_at <= cap_at else "cap"
        if mode == "clock":
            fire_at = clock_at if clock_at <= cap_at else cap_at
            if times[k] > fire_at:
                return close_at(fire_at, max(eidx, last_buy_i), "clock")
            if times[k] >= cap_at:
                return close_at(cap_at, k, "cap")
            if is_buy[k]:
                last_buy_ts = times[k]
                last_buy_i = k
            continue

        if times[k] > fire_gap:
            if mode == "gap" or (mode == "trail" and not armed):
                return close_at(fire_gap, last_buy_i, gap_reason)
            # armed trail: a pause is not the exit; keep walking to trail or cap

        p = px[k]
        if p > 0 and mode == "trail":
            if not armed:
                if p >= arm_lvl:
                    armed = True
                    peak = p
            else:
                if p > peak:
                    peak = p
                elif p <= peak * trail_keep:
                    return close_at(times[k], k, "trail")

        if times[k] >= cap_at:
            return close_at(cap_at, k, "cap")

        if is_buy[k]:
            last_buy_ts = times[k]
            last_buy_i = k

    if mode == "clock":
        fire_at = clock_at if clock_at <= cap_at else cap_at
        return close_at(fire_at, last_buy_i, "clock")

    gap_at = last_buy_ts + gap_delay
    fire_gap = gap_at if gap_at <= cap_at else cap_at
    reason = "gap" if gap_at <= cap_at else "cap"
    if mode == "trail" and armed:
        reason = "cap"
    return close_at(fire_gap, last_buy_i, reason)


def q(conn, sql):
    with conn.cursor() as cur:
        cur.execute(sql)
        cols = [d[0] for d in cur.description]
        return pd.DataFrame.from_records(cur.fetchall(), columns=cols)


def load(conn):
    tape = q(
        conn,
        """
        SELECT mint, slot, tx_index, ts, trade_type, px, vsol_lp
        FROM ixg.tape
        WHERE mint IN (
            SELECT mint FROM ixg.cand
            UNION
            SELECT mint FROM ixg.dead_cand
        )
          AND px IS NOT NULL AND vsol_lp IS NOT NULL AND vsol_lp > 0
        ORDER BY mint, slot, tx_index
        """,
    )
    work = q(conn, "SELECT mint, slot, tx_index, ts FROM ixg.cand ORDER BY mint, ts")
    dead = q(conn, "SELECT mint, slot, tx_index, ts FROM ixg.dead_cand ORDER BY mint, ts")
    return tape, work, dead


def index_events(tape_g, events):
    """Map each event to a row index in that mint's tape. -1 if missing."""
    out = []
    for mint, grp in events.groupby("mint", sort=False):
        if mint not in tape_g:
            for _ in range(len(grp)):
                out.append(-1)
            continue
        t = tape_g[mint]
        slots = t["slot"].to_numpy()
        txs = t["tx_index"].to_numpy()
        # events may not match exactly if tape filtered px-null trigger rows
        key = {(int(s), int(x)): i for i, (s, x) in enumerate(zip(slots, txs))}
        for s, x in zip(grp["slot"].to_numpy(), grp["tx_index"].to_numpy()):
            out.append(key.get((int(s), int(x)), -1))
    return np.array(out, dtype=np.int64)


def run_book(tape_g, ev, lag_ms, mode, policy, clock_s=None, death_s=None):
    """policy: 'first' | 'reentry'."""
    rows = []
    ev = ev.copy()
    ev["_i"] = index_events(tape_g, ev)
    for mint, grp in ev.groupby("mint", sort=False):
        t = tape_g.get(mint)
        if t is None:
            continue
        times = t["ts"].to_numpy(dtype="datetime64[ns]")
        slots = t["slot"].to_numpy()
        px = t["px"].to_numpy()
        vsol = t["vsol_lp"].to_numpy(dtype=np.float64)
        is_buy = (t["trade_type"].to_numpy() == "buy")
        n = len(t)
        last_exit = None
        for rec, idx in zip(grp.itertuples(index=False), grp["_i"].to_numpy()):
            if idx < 0:
                continue
            ts = np.datetime64(rec.ts, "ns")
            if policy == "first" and last_exit is not None:
                break
            if policy == "reentry" and last_exit is not None and ts < last_exit:
                continue
            got = walk_one(
                times, slots, px, vsol, is_buy, (int(idx), ts, int(rec.slot)),
                lag_ms, mode, clock_s=clock_s, death_s=death_s,
            )
            if got is None:
                continue
            r, hold, reason, xts = got
            rows.append((mint, rec.ts, r, hold, reason))
            last_exit = xts
            if policy == "first":
                break
    if not rows:
        return pd.DataFrame(columns=["mint", "ts", "net", "hold", "reason"])
    return pd.DataFrame(rows, columns=["mint", "ts", "net", "hold", "reason"])


def summarize(df, label):
    if df.empty:
        print(f"{label}: empty")
        return
    n = len(df)
    mints = df["mint"].nunique()
    mean = df["net"].mean()
    med = df["net"].median()
    win = (df["net"] > 0).mean()
    sol = (0.10 * df["net"]).sum()
    hold = df["hold"].median()
    days = df["ts"].dt.date if hasattr(df["ts"].dt, "date") else pd.to_datetime(df["ts"]).dt.date
    d = pd.DataFrame({"d": pd.to_datetime(df["ts"]).dt.date, "net": df["net"], "sol": 0.10 * df["net"]})
    by = d.groupby("d").agg(n=("net", "size"), mean=("net", "mean"), sol=("sol", "sum"))
    npos = int((by["mean"] > 0).sum())
    nd = len(by)
    # drop 08-17 sensitivity
    by2 = by[by.index.astype(str) != "2026-08-17"]
    reasons = df["reason"].value_counts().to_dict()
    print(
        f"{label}: n={n} mints={mints} mean={100*mean:.3f}% med={100*med:.3f}% "
        f"win={100*win:.1f}% sol={sol:.2f} hold_med={hold:.1f}s days+={npos}/{nd} "
        f"ex08-17_sol={by2['sol'].sum():.2f} reasons={reasons}"
    )
    # IS / OOS
    ts = pd.to_datetime(df["ts"])
    is_ = df[ts < "2026-08-17"]
    oos = df[ts >= "2026-08-18"]
    if len(is_):
        print(
            f"  IS 08-11..16: n={len(is_)} mean={100*is_['net'].mean():.3f}% "
            f"sol={0.10*is_['net'].sum():.2f} days+="
            f"{(is_.assign(d=pd.to_datetime(is_['ts']).dt.date).groupby('d')['net'].mean()>0).sum()}"
        )
    if len(oos):
        print(
            f"  OOS 08-18..22: n={len(oos)} mean={100*oos['net'].mean():.3f}% "
            f"sol={0.10*oos['net'].sum():.2f} days+="
            f"{(oos.assign(d=pd.to_datetime(oos['ts']).dt.date).groupby('d')['net'].mean()>0).sum()}"
        )
    print("  by day:", " ".join(f"{i} {100*r['mean']:.1f}%/{r['sol']:.1f}" for i, r in by.iterrows()))


def main():
    url = os.environ["DATABASE_URL"]
    conn = psycopg2.connect(url)
    print("loading...", flush=True)
    tape, work, dead = load(conn)
    conn.close()
    print(f"tape={len(tape)} work={len(work)} dead={len(dead)}", flush=True)
    tape_g = {m: g.reset_index(drop=True) for m, g in tape.groupby("mint", sort=False)}

    jobs = [
        ("work clock20 0ms first", work, 0, "clock", "first"),
        ("work clock20 95ms first", work, 95, "clock", "first"),
        ("work clock20 95ms reentry", work, 95, "clock", "reentry"),
        ("dead clock20 95ms first", dead, 95, "clock", "first"),
    ]
    for label, ev, lag, mode, pol in jobs:
        print(f"running {label}...", flush=True)
        df = run_book(tape_g, ev, lag, mode, pol)
        summarize(df, label)


if __name__ == "__main__":
    sys.stdout.reconfigure(encoding="utf-8")
    main()
