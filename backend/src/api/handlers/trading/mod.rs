// All trading handlers (cashback + on-chain Solana queries) moved to
// `backend-deploy`; re-export so existing `handlers::trading::…` paths resolve.
pub use backend_deploy::api::handlers::trading::*;
