use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Lifecycle state of a paper-test run.
/// - `Running`  — accepting new positions; cap not yet reached or positions still open.
/// - `Finished` — total-token cap reached AND every position exited (auto-deactivated + notified).
/// - `Stopped`  — the rule was manually deactivated before finishing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum PaperRunStatus {
    Running,
    Finished,
    Stopped,
}

impl PaperRunStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "Running",
            Self::Finished => "Finished",
            Self::Stopped => "Stopped",
        }
    }
}

impl std::str::FromStr for PaperRunStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Running" => Ok(Self::Running),
            "Finished" => Ok(Self::Finished),
            "Stopped" => Ok(Self::Stopped),
            other => Err(format!("Unknown paper run status: {other}")),
        }
    }
}

/// One paper-test run for a TPSL rule. Only the latest run per rule is retained.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaperRun {
    pub id: Uuid,
    pub rule_id: Uuid,
    pub run_seq: i64,
    pub status: PaperRunStatus,
    /// Total-token cap snapshot at run start (`None` = unlimited).
    pub max_total_tokens: Option<u64>,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
}
