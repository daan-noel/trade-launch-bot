-- Phase 2.F — persist the orchestrator `Plan` + fingerprint `AuditReport` next to
-- the execution rows, so every write flow (launch bundle, manage action) can be
-- previewed and replayed from the exact plan that was audited and executed.
--
-- `plan`  = the serialized `orchestrator::Plan` (ops + funding graph + schedule),
--           the SSOT the executor rebuilds and the disguises are recomputed from.
-- `audit` = the serialized `orchestrator::AuditReport` (findings + score +
--           hard_reject) captured at the gate, for the operator to inspect after
--           the fact. Write-only (display); execution never reads it back.
--
-- Both nullable: pre-cutover rows (and any legacy row) simply carry NULL. The
-- old `bundles.legs` / `manage_actions.plan` columns stay populated for the
-- confirm watcher + existing UI, so this is purely additive.

ALTER TABLE bundles        ADD COLUMN IF NOT EXISTS plan  JSONB;
ALTER TABLE bundles        ADD COLUMN IF NOT EXISTS audit JSONB;

ALTER TABLE manage_actions ADD COLUMN IF NOT EXISTS plan_orchestrator JSONB;
ALTER TABLE manage_actions ADD COLUMN IF NOT EXISTS audit             JSONB;
