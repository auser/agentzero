-- AgentZero Autopilot Schema
-- Apply via: supabase db push  (or paste into Supabase SQL Editor)

-- ---------------------------------------------------------------------------
-- proposals
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS proposals (
    id          TEXT PRIMARY KEY,
    agent_id    TEXT NOT NULL,
    title       TEXT NOT NULL,
    description TEXT NOT NULL,
    proposal_type TEXT NOT NULL CHECK (proposal_type IN ('content_idea', 'task_request', 'resource_request', 'system_change')),
    priority    TEXT NOT NULL DEFAULT 'medium' CHECK (priority IN ('low', 'medium', 'high', 'critical')),
    estimated_cost_microdollars BIGINT NOT NULL DEFAULT 0,
    status      TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'approved', 'rejected', 'executed')),
    cap_gate_result JSONB,
    metadata    JSONB NOT NULL DEFAULT '{}',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_proposals_status ON proposals (status);
CREATE INDEX idx_proposals_agent ON proposals (agent_id);
CREATE INDEX idx_proposals_created ON proposals (created_at DESC);

-- ---------------------------------------------------------------------------
-- missions
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS missions (
    id              TEXT PRIMARY KEY,
    proposal_id     TEXT NOT NULL REFERENCES proposals(id),
    title           TEXT NOT NULL,
    assigned_agent  TEXT NOT NULL,
    status          TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'in_progress', 'completed', 'failed', 'stalled')),
    heartbeat_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    deadline        TIMESTAMPTZ,
    result          JSONB,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_missions_status ON missions (status);
CREATE INDEX idx_missions_agent ON missions (assigned_agent);
CREATE INDEX idx_missions_heartbeat ON missions (heartbeat_at);

-- ---------------------------------------------------------------------------
-- mission_steps
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS mission_steps (
    id              TEXT PRIMARY KEY DEFAULT gen_random_uuid()::text,
    mission_id      TEXT NOT NULL REFERENCES missions(id) ON DELETE CASCADE,
    step_index      INTEGER NOT NULL,
    description     TEXT NOT NULL,
    agent_id        TEXT NOT NULL,
    status          TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'in_progress', 'completed', 'failed', 'skipped')),
    result          TEXT,
    started_at      TIMESTAMPTZ,
    completed_at    TIMESTAMPTZ,
    UNIQUE (mission_id, step_index)
);

CREATE INDEX idx_mission_steps_mission ON mission_steps (mission_id);

-- ---------------------------------------------------------------------------
-- events
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS events (
    id              TEXT PRIMARY KEY,
    event_type      TEXT NOT NULL,
    source_agent    TEXT NOT NULL,
    payload         JSONB NOT NULL DEFAULT '{}',
    correlation_id  TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_events_type ON events (event_type);
CREATE INDEX idx_events_source ON events (source_agent);
CREATE INDEX idx_events_created ON events (created_at DESC);
CREATE INDEX idx_events_correlation ON events (correlation_id) WHERE correlation_id IS NOT NULL;

-- ---------------------------------------------------------------------------
-- triggers
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS triggers (
    id              TEXT PRIMARY KEY,
    name            TEXT NOT NULL,
    condition_type  TEXT NOT NULL CHECK (condition_type IN ('event_match', 'cron', 'metric_threshold')),
    condition_config JSONB NOT NULL DEFAULT '{}',
    action_type     TEXT NOT NULL CHECK (action_type IN ('propose_task', 'notify_agent', 'run_pipeline')),
    action_config   JSONB NOT NULL DEFAULT '{}',
    cooldown_secs   BIGINT NOT NULL DEFAULT 0,
    last_fired_at   TIMESTAMPTZ,
    enabled         BOOLEAN NOT NULL DEFAULT true
);

-- ---------------------------------------------------------------------------
-- content
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS content (
    id              TEXT PRIMARY KEY DEFAULT gen_random_uuid()::text,
    content_type    TEXT NOT NULL,
    title           TEXT NOT NULL,
    slug            TEXT UNIQUE NOT NULL,
    body            TEXT NOT NULL DEFAULT '',
    status          TEXT NOT NULL DEFAULT 'draft' CHECK (status IN ('draft', 'review', 'published', 'archived')),
    author_agent    TEXT NOT NULL,
    mission_id      TEXT REFERENCES missions(id),
    metadata        JSONB NOT NULL DEFAULT '{}',
    published_at    TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_content_status ON content (status);
CREATE INDEX idx_content_slug ON content (slug);
CREATE INDEX idx_content_published ON content (published_at DESC) WHERE published_at IS NOT NULL;

-- ---------------------------------------------------------------------------
-- agent_activity
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS agent_activity (
    id              TEXT PRIMARY KEY DEFAULT gen_random_uuid()::text,
    agent_id        TEXT NOT NULL,
    activity_type   TEXT NOT NULL,
    summary         TEXT NOT NULL DEFAULT '',
    metadata        JSONB NOT NULL DEFAULT '{}',
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_agent_activity_agent ON agent_activity (agent_id);
CREATE INDEX idx_agent_activity_created ON agent_activity (created_at DESC);

-- ---------------------------------------------------------------------------
-- cap_gate_ledger
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS cap_gate_ledger (
    id                  TEXT PRIMARY KEY DEFAULT gen_random_uuid()::text,
    agent_id            TEXT NOT NULL,
    cost_microdollars   BIGINT NOT NULL DEFAULT 0,
    mission_id          TEXT REFERENCES missions(id),
    recorded_at         TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_cap_gate_ledger_agent ON cap_gate_ledger (agent_id);
CREATE INDEX idx_cap_gate_ledger_recorded ON cap_gate_ledger (recorded_at DESC);

-- ---------------------------------------------------------------------------
-- Row-Level Security
-- ---------------------------------------------------------------------------

-- Enable RLS on all tables
ALTER TABLE proposals ENABLE ROW LEVEL SECURITY;
ALTER TABLE missions ENABLE ROW LEVEL SECURITY;
ALTER TABLE mission_steps ENABLE ROW LEVEL SECURITY;
ALTER TABLE events ENABLE ROW LEVEL SECURITY;
ALTER TABLE triggers ENABLE ROW LEVEL SECURITY;
ALTER TABLE content ENABLE ROW LEVEL SECURITY;
ALTER TABLE agent_activity ENABLE ROW LEVEL SECURITY;
ALTER TABLE cap_gate_ledger ENABLE ROW LEVEL SECURITY;

-- Service role: full access (agentzero VPS)
CREATE POLICY "service_role_full_access" ON proposals FOR ALL TO service_role USING (true) WITH CHECK (true);
CREATE POLICY "service_role_full_access" ON missions FOR ALL TO service_role USING (true) WITH CHECK (true);
CREATE POLICY "service_role_full_access" ON mission_steps FOR ALL TO service_role USING (true) WITH CHECK (true);
CREATE POLICY "service_role_full_access" ON events FOR ALL TO service_role USING (true) WITH CHECK (true);
CREATE POLICY "service_role_full_access" ON triggers FOR ALL TO service_role USING (true) WITH CHECK (true);
CREATE POLICY "service_role_full_access" ON content FOR ALL TO service_role USING (true) WITH CHECK (true);
CREATE POLICY "service_role_full_access" ON agent_activity FOR ALL TO service_role USING (true) WITH CHECK (true);
CREATE POLICY "service_role_full_access" ON cap_gate_ledger FOR ALL TO service_role USING (true) WITH CHECK (true);

-- Anon (dashboard): read-only on all tables
CREATE POLICY "anon_read" ON proposals FOR SELECT TO anon USING (true);
CREATE POLICY "anon_read" ON missions FOR SELECT TO anon USING (true);
CREATE POLICY "anon_read" ON mission_steps FOR SELECT TO anon USING (true);
CREATE POLICY "anon_read" ON events FOR SELECT TO anon USING (true);
CREATE POLICY "anon_read" ON triggers FOR SELECT TO anon USING (true);
CREATE POLICY "anon_read" ON content FOR SELECT TO anon USING (true);
CREATE POLICY "anon_read" ON agent_activity FOR SELECT TO anon USING (true);
CREATE POLICY "anon_read" ON cap_gate_ledger FOR SELECT TO anon USING (true);

-- Public: read-only on published content only
CREATE POLICY "public_read_published" ON content FOR SELECT TO authenticated USING (status = 'published');

-- ---------------------------------------------------------------------------
-- Realtime
-- ---------------------------------------------------------------------------
ALTER PUBLICATION supabase_realtime ADD TABLE proposals;
ALTER PUBLICATION supabase_realtime ADD TABLE missions;
ALTER PUBLICATION supabase_realtime ADD TABLE mission_steps;
ALTER PUBLICATION supabase_realtime ADD TABLE events;
ALTER PUBLICATION supabase_realtime ADD TABLE agent_activity;
ALTER PUBLICATION supabase_realtime ADD TABLE content;

-- ---------------------------------------------------------------------------
-- Helper views
-- ---------------------------------------------------------------------------
CREATE OR REPLACE VIEW daily_spend AS
SELECT
    date_trunc('day', recorded_at) AS day,
    agent_id,
    SUM(cost_microdollars) AS total_microdollars
FROM cap_gate_ledger
GROUP BY 1, 2;

CREATE OR REPLACE VIEW active_missions AS
SELECT m.*, p.proposal_type, p.priority
FROM missions m
JOIN proposals p ON p.id = m.proposal_id
WHERE m.status IN ('pending', 'in_progress');
