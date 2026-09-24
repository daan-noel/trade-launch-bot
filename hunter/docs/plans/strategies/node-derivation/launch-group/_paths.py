"""Bootstrap for the launch-group scripts: toolkit + study-kernel on sys.path.

D is a creation ix sequence. CU limit/price is a split, not the identity: the same
bot can change those presets without changing the instruction list.
"""
from __future__ import annotations

import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

import toolkit  # noqa: E402,F401
from toolkit.paths import DATA, HUNTER, LAKE, LOCAL, SHARED  # noqa: E402,F401
from toolkit.paths import data as data_file  # noqa: E402,F401

NODE_NAME = "launch-group"
SEQ6_BUYV2 = (
    '["Compute Budget: SetComputeUnitLimit","Compute Budget: SetComputeUnitPrice",'
    '"Pump.Fun: Create_v2","Associated Token: CreateIdempotent",'
    '"Pump.Fun: BuyV2","System Program: Transfer"]'
)
SEQ5_BUYV2 = (
    '["Compute Budget: SetComputeUnitLimit","Compute Budget: SetComputeUnitPrice",'
    '"Pump.Fun: Create_v2","Associated Token: CreateIdempotent","Pump.Fun: BuyV2"]'
)
SEQ3_BUYV2 = (
    '["Pump.Fun: Create_v2","Associated Token: CreateIdempotent","Pump.Fun: BuyV2"]'
)
SEQ6_BUY = (
    '["Compute Budget: SetComputeUnitLimit","Compute Budget: SetComputeUnitPrice",'
    '"Pump.Fun: Create_v2","Associated Token: CreateIdempotent",'
    '"Pump.Fun: Buy","System Program: Transfer"]'
)
