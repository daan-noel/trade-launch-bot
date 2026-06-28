//! Deploy strategy handlers. Per the T11/T12 split (Option A), the tpsl1/tpsl2
//! rule CRUD + simulate handlers (which also take `LocalState`) stay in `backend`
//! / later `lab`; only the live position read handlers move here.

pub mod tpsl1_positions;
pub mod tpsl2_positions;
