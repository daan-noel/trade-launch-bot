"""Sealed lake days converted to the study tape's exact format: a holdout tape.

  grain      the LAST leg of each (mint, slot, tx_index), as the study tape's source keeps it;
             --all-legs keeps every leg in leg order (the grain a live engine meets)
  t_ms       block_time (us) / 1000, rounded
  reserve    vsol after the print, in lamports; curve rows only, rows with no vsol dropped
  side       1 buy / -1 sell
  build      build_core: md5 of the ix_labels joined by "|", after dropping
             "Associated Token: Create*", "*: CloseAccount" and "Memo Program*" (5,000 of 5,000
             sampled rows match the study tape)
  wallet_id  an integer code per address; the address map is written beside it

Start the export a few days before the first fire time (a WARM-UP) so every coin carries its
holder book and seller history from its first print. Check the rows against the study tape on
the days the two overlap before trusting it.

  python -m toolkit.lake_export PREFIX DAY [DAY ...] [--all-legs]
      writes data/PREFIX_prints.parquet, PREFIX_wallets.parquet, PREFIX_tok.parquet;
      register the tape in tapes.TAPES with its first fire time
"""
from __future__ import annotations

import hashlib
import json
import sys
import time

import numpy as np
import pandas as pd
import pyarrow as pa
import pyarrow.parquet as pq

from .paths import DATA, LAKE

COLS = ["mint", "is_buy", "sol_amount", "slot", "block_time", "leg_index", "vsol", "tx_index",
        "ix_labels", "wallet", "venue"]


def build_core(labels):
    if labels is None:
        return ""
    keep = [x for x in json.loads(labels)
            if not (x.startswith("Associated Token: Create") or x.endswith(": CloseAccount")
                    or x.startswith("Memo Program"))]
    return hashlib.md5("|".join(keep).encode()).hexdigest()


def day_frame(day: str, all_legs: bool = False) -> pd.DataFrame:
    d = pq.read_table(str(LAKE / "trades" / f"dt={day}" / "data.parquet"), columns=COLS).to_pandas()
    d = d[d.vsol.notna() & (d.vsol > 0) & (d.venue == "curve")]
    if all_legs:
        d = d.sort_values(["mint", "slot", "tx_index", "leg_index"])
    else:
        d = d.sort_values(["mint", "slot", "tx_index", "leg_index"],
                          ascending=[True, True, True, False])
        d = d.drop_duplicates(["mint", "slot", "tx_index"], keep="first")
    codes, uniq = pd.factorize(d.ix_labels)
    cores = np.array([build_core(u) for u in uniq], dtype=object)
    d["build"] = np.where(codes >= 0, cores[np.maximum(codes, 0)], "")
    return pd.DataFrame({
        "mint": d.mint.values, "slot": d.slot.values, "tx_index": d.tx_index.values,
        "leg_index": d.leg_index.values,
        "t_ms": ((d.block_time.values + 500) // 1000).astype(np.int64),
        "reserve_lamports": np.round(d.vsol.values * 1e9).astype(np.int64),
        "amount_lamports": np.round(d.sol_amount.values * 1e9).astype(np.int64),
        "side": np.where(d.is_buy.values, 1, -1).astype(np.int16),
        "wallet": d.wallet.values, "build": d.build.values})


def export(prefix: str, days: list[str], all_legs: bool = False) -> None:
    t0 = time.time()
    parts = []
    for day in days:
        parts.append(day_frame(day, all_legs))
        print("  %s  %s rows  %ds" % (day, f"{len(parts[-1]):,}", time.time() - t0), flush=True)
    D = pd.concat(parts, ignore_index=True)
    D = D.sort_values(["mint", "slot", "tx_index", "leg_index"], kind="stable").reset_index(drop=True)
    wid, waddr = pd.factorize(D.wallet)
    D["wallet_id"] = wid.astype(np.int64)
    D = D.drop(columns=["wallet"])
    DATA.mkdir(exist_ok=True)
    pq.write_table(pa.Table.from_pandas(D, preserve_index=False),
                   str(DATA / f"{prefix}_prints.parquet"), compression="zstd")
    pq.write_table(pa.table({"wallet_id": np.arange(len(waddr), dtype=np.int64),
                             "address": np.asarray(waddr, dtype=object)}),
                   str(DATA / f"{prefix}_wallets.parquet"), compression="zstd")
    tk = pq.read_table(str(LAKE / "tokens" / "tokens.parquet"),
                       columns=["mint", "created_at"]).to_pandas()
    tk["created_at"] = pd.to_datetime(tk.created_at, unit="us", utc=True).dt.strftime(
        "%Y-%m-%dT%H:%M:%S.%fZ")
    pq.write_table(pa.Table.from_pandas(tk, preserve_index=False),
                   str(DATA / f"{prefix}_tok.parquet"), compression="zstd")
    print("%s: %s rows, %s mints, %s wallets  %ds" % (prefix, f"{len(D):,}", f"{D.mint.nunique():,}",
                                                     f"{len(waddr):,}", time.time() - t0))


if __name__ == "__main__":
    args = [a for a in sys.argv[2:] if a != "--all-legs"]
    export(sys.argv[1], args, all_legs="--all-legs" in sys.argv[2:])
