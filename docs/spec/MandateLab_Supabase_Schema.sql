-- MandateLab proposed off-chain audit persistence — P1, not required for P0.
-- NEAR testnet contract remains source of truth for funds and payment status.
-- Review and adapt to the actual Supabase project before applying.
-- Public demo contains synthetic scenarios only. Do not persist secrets/raw private docs.

CREATE EXTENSION IF NOT EXISTS pgcrypto;

CREATE TABLE IF NOT EXISTS scenarios (
  id TEXT PRIMARY KEY CHECK (length(id) BETWEEN 1 AND 80),
  title TEXT NOT NULL CHECK (length(title) BETWEEN 1 AND 160),
  category TEXT NOT NULL,
  invoice_text TEXT NOT NULL CHECK (length(invoice_text) <= 8000),
  expected_outcome TEXT NOT NULL,
  enabled BOOLEAN NOT NULL DEFAULT FALSE,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS runs (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  scenario_id TEXT NOT NULL REFERENCES scenarios(id),
  network TEXT NOT NULL DEFAULT 'testnet' CHECK (network = 'testnet'),
  contract_id TEXT NOT NULL,
  policy_version INTEGER NOT NULL DEFAULT 1 CHECK (policy_version >= 1),
  started_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  finished_at TIMESTAMPTZ,
  status TEXT NOT NULL DEFAULT 'QUEUED' CHECK
    (status IN ('QUEUED', 'AGENT_PROPOSED', 'SUBMITTED', 'PENDING', 'BLOCKED_BY_MODEL',
      'BLOCKED_BY_CONTRACT', 'SUCCEEDED_ON_CHAIN', 'FAILED_NETWORK_OR_TRANSFER', 'ERROR')),
  input_sha256 CHAR(64) NOT NULL CHECK (input_sha256 ~ '^[0-9a-f]{64}$'),
  model_id TEXT,
  gateway_request_id TEXT,
  CONSTRAINT runs_time_order CHECK (finished_at IS NULL OR finished_at >= started_at)
);

CREATE TABLE IF NOT EXISTS payment_attempts (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  run_id UUID NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
  request_id TEXT NOT NULL UNIQUE CHECK (length(request_id) BETWEEN 8 AND 128),
  agent_account_id TEXT NOT NULL,
  recipient_id TEXT,
  amount_yocto TEXT CHECK (amount_yocto IS NULL OR
    (length(amount_yocto) BETWEEN 1 AND 39 AND amount_yocto ~ '^[0-9]+$')),
  context_sha256 CHAR(64) NOT NULL CHECK (context_sha256 ~ '^[0-9a-f]{64}$'),
  model_intent TEXT CHECK (model_intent IS NULL OR model_intent IN ('pay','reject','clarify')),
  policy_decision TEXT,
  execution_status TEXT NOT NULL DEFAULT 'PROPOSED',
  tx_hash TEXT,
  error_code TEXT,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  finalized_at TIMESTAMPTZ,
  CONSTRAINT payment_attempts_time_order
    CHECK (finalized_at IS NULL OR finalized_at >= created_at)
);

CREATE TABLE IF NOT EXISTS test_evidence (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  attempt_id UUID NOT NULL REFERENCES payment_attempts(id) ON DELETE CASCADE,
  evidence_type TEXT NOT NULL,
  rpc_result JSONB NOT NULL DEFAULT '{}'::jsonb,
  block_height TEXT,
  captured_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  verified BOOLEAN NOT NULL DEFAULT FALSE
);

CREATE INDEX IF NOT EXISTS idx_runs_started_at ON runs (started_at DESC);
CREATE INDEX IF NOT EXISTS idx_runs_scenario_id ON runs (scenario_id);
CREATE INDEX IF NOT EXISTS idx_attempts_run_id ON payment_attempts (run_id);
CREATE INDEX IF NOT EXISTS idx_attempts_tx_hash ON payment_attempts (tx_hash) WHERE tx_hash IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_evidence_attempt_id ON test_evidence (attempt_id);

-- Default-deny on all tables at database API boundary.
-- Use server-only service credentials for inserts and sanitized reads.
ALTER TABLE scenarios ENABLE ROW LEVEL SECURITY;
ALTER TABLE runs ENABLE ROW LEVEL SECURITY;
ALTER TABLE payment_attempts ENABLE ROW LEVEL SECURITY;
ALTER TABLE test_evidence ENABLE ROW LEVEL SECURITY;

-- Only explicitly enabled synthetic fixtures may be publicly read.
-- Do not grant public SELECT on runs, attempts or raw evidence.
DROP POLICY IF EXISTS public_scenarios_read ON scenarios;
CREATE POLICY public_scenarios_read ON scenarios FOR SELECT TO anon, authenticated
USING (enabled = TRUE);

-- NOTE: Supabase API access grants and any account-scoped write policies
-- must be configured explicitly for your deployment. Never ship service_role
-- credentials in a public Next.js client or browser bundle.
