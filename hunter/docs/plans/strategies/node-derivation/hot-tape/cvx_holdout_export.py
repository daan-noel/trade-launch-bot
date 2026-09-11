"""The HOLDOUT tape: the lake's sealed days converted to the study tape's exact format.

The study tape (`cvx_prints.parquet`, from aa.pxf) ends 2026-09-06 11:59:59 UTC. Every threshold
on the hot-tape sentence (evidence 1.12-1.14) was read off it. This file builds the days after it
from the parquet lake, with the same row grain and the same columns, so the frozen sentence can be
booked on them unchanged:

  grain      the LAST leg of each (mint, slot, tx_index), as aa.pxf's DISTINCT ON keeps it
  t_ms       block_time (us) / 1000, rounded, as aa.pxf's EXTRACT(EPOCH) * 1000 cast rounds
  reserve    vsol after the print, in lamports; curve rows only, and rows with no vsol dropped,
             as aa.pxf holds them
  side       1 buy / -1 sell
  build      aa.pxf's build_core, recovered exactly (5,000 of 5,000 sampled rows):
             md5 of the ix_labels joined by "|", after dropping "Associated Token: Create*",
             "*: CloseAccount" and "Memo Program*"
  wallet_id  an integer code per address (the lake carries addresses, not wallet_dict ids); the
             address map is written beside it so the six node wallets can be dropped by address

Warm-up: the tape starts WARMUP so every coin traded after the holdout start has its holder book
and seller history from its first print; coins created before WARMUP are flagged, not dropped.
"""
from __future__ import annotations

import _paths  # noqa: F401  - the toolkit and study-kernel on sys.path
from _paths import data_file

import hashlib
import json
import sys
import time
from pathlib import Path

import numpy as np
import pandas as pd
import pyarrow as pa
import pyarrow.parquet as pq

LAKE = _paths.LAKE
OUT = data_file("cvx_holdout_prints.parquet")
WMAP = data_file("cvx_holdout_wallets.parquet")
TOK = data_file("cvx_holdout_tok.parquet")
DAYS = ["2026-09-03", "2026-09-04", "2026-09-05", "2026-09-06", "2026-09-07", "2026-09-08",
        "2026-09-09", "2026-09-10"]
COLS = ["mint", "is_buy", "sol_amount", "slot", "block_time", "leg_index", "vsol", "tx_index",
        "ix_labels", "wallet", "venue"]


def build_core(labels: str | None) -> str:
    if labels is None:
        return ""
    keep = [x for x in json.loads(labels)
            if not (x.startswith("Associated Token: Create") or x.endswith(": CloseAccount")
                    or x.startswith("Memo Program"))]
    return hashlib.md5("|".join(keep).encode()).hexdigest()


def day_frame(day: str) -> pd.DataFrame:
    d = pq.read_table(str(LAKE / "trades" / f"dt={day}" / "data.parquet"), columns=COLS).to_pandas()
    d = d[d.vsol.notna() & (d.vsol > 0) & (d.venue == "curve")]
    d = d.sort_values(["mint", "slot", "tx_index", "leg_index"], ascending=[True, True, True, False])
    d = d.drop_duplicates(["mint", "slot", "tx_index"], keep="first")
    codes, uniq = pd.factorize(d.ix_labels)
    cores = np.array([build_core(u) for u in uniq], dtype=object)
    d["build"] = np.where(codes >= 0, cores[np.maximum(codes, 0)], "")
    return pd.DataFrame({
        "mint": d.mint.values, "slot": d.slot.values, "tx_index": d.tx_index.values,
        "t_ms": ((d.block_time.values + 500) // 1000).astype(np.int64),
        "reserve_lamports": np.round(d.vsol.values * 1e9).astype(np.int64),
        "amount_lamports": np.round(d.sol_amount.values * 1e9).astype(np.int64),
        "side": np.where(d.is_buy.values, 1, -1).astype(np.int16),
        "wallet": d.wallet.values, "build": d.build.values})


def main() -> None:
    t0 = time.time()
    days = sys.argv[1:] or DAYS
    parts = []
    for day in days:
        f = day_frame(day)
        parts.append(f)
        print("  %s  %s rows  %ds" % (day, f"{len(f):,}", time.time() - t0), flush=True)
    D = pd.concat(parts, ignore_index=True)
    D = D.sort_values(["mint", "slot", "tx_index"], kind="stable").reset_index(drop=True)
    wid, waddr = pd.factorize(D.wallet)
    D["wallet_id"] = wid.astype(np.int64)
    D = D.drop(columns=["wallet"])
    pq.write_table(pa.Table.from_pandas(D, preserve_index=False), str(OUT), compression="zstd")
    pq.write_table(pa.table({"wallet_id": np.arange(len(waddr), dtype=np.int64),
                             "address": np.asarray(waddr, dtype=object)}), str(WMAP),
                   compression="zstd")
    tk = pq.read_table(str(LAKE / "tokens" / "tokens.parquet"), columns=["mint", "created_at"])
    tk = tk.to_pandas()
    tk["created_at"] = pd.to_datetime(tk.created_at, unit="us", utc=True).dt.strftime(
        "%Y-%m-%dT%H:%M:%S.%fZ")
    pq.write_table(pa.Table.from_pandas(tk, preserve_index=False), str(TOK), compression="zstd")
    print("holdout tape %s rows, %s mints, %s wallets  %ds"
          % (f"{len(D):,}", f"{D.mint.nunique():,}", f"{len(waddr):,}", time.time() - t0),
          flush=True)


if __name__ == "__main__":
    main()
