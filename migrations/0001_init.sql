CREATE EXTENSION IF NOT EXISTS pg_trgm;
CREATE EXTENSION IF NOT EXISTS vector;

CREATE TABLE developers (
  id UUID PRIMARY KEY,
  login_name TEXT NOT NULL UNIQUE CHECK (char_length(login_name) BETWEEN 3 AND 64),
  password_hash TEXT NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE workspaces (
  id UUID PRIMARY KEY,
  developer_id UUID REFERENCES developers(id) ON DELETE SET NULL,
  name TEXT NOT NULL CHECK (char_length(name) BETWEEN 1 AND 120),
  claim_token_hash TEXT UNIQUE,
  invite_token_hash TEXT UNIQUE,
  publication_policy TEXT NOT NULL DEFAULT 'manual' CHECK (publication_policy IN ('manual', 'auto')),
  created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE developer_sessions (
  id UUID PRIMARY KEY,
  developer_id UUID NOT NULL REFERENCES developers(id) ON DELETE CASCADE,
  token_hash TEXT NOT NULL UNIQUE,
  expires_at TIMESTAMPTZ NOT NULL,
  revoked_at TIMESTAMPTZ,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE agents (
  id UUID PRIMARY KEY,
  workspace_id UUID NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
  name TEXT NOT NULL CHECK (char_length(name) BETWEEN 1 AND 120),
  revoked_at TIMESTAMPTZ,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE agent_keys (
  id UUID PRIMARY KEY,
  agent_id UUID NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
  key_prefix TEXT NOT NULL CHECK (char_length(key_prefix) <= 32),
  key_hash TEXT NOT NULL UNIQUE,
  revoked_at TIMESTAMPTZ,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE memories (
  id UUID PRIMARY KEY,
  workspace_id UUID NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
  author_agent_id UUID NOT NULL REFERENCES agents(id) ON DELETE RESTRICT,
  visibility TEXT NOT NULL DEFAULT 'agent_private' CHECK (visibility IN ('agent_private', 'developer_shared', 'public')),
  problem TEXT NOT NULL CHECK (char_length(problem) BETWEEN 2 AND 1000),
  conditions JSONB NOT NULL DEFAULT '{}'::jsonb,
  action TEXT NOT NULL CHECK (char_length(action) BETWEEN 1 AND 2400),
  outcome TEXT NOT NULL CHECK (char_length(outcome) BETWEEN 1 AND 2400),
  outcome_kind TEXT NOT NULL CHECK (outcome_kind IN ('success', 'failure', 'partial', 'unknown')),
  source_type TEXT NOT NULL DEFAULT 'agent' CHECK (source_type IN ('agent', 'human', 'public_import')),
  language TEXT NOT NULL DEFAULT 'zh-CN' CHECK (char_length(language) BETWEEN 2 AND 20),
  tags TEXT[] NOT NULL DEFAULT '{}',
  search_text TEXT NOT NULL,
  embedding vector,
  publication_requested_at TIMESTAMPTZ,
  published_at TIMESTAMPTZ,
  removed_at TIMESTAMPTZ,
  removed_reason TEXT,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX memories_scope_idx ON memories (visibility, workspace_id, author_agent_id) WHERE removed_at IS NULL;
CREATE INDEX memories_created_idx ON memories (created_at DESC) WHERE removed_at IS NULL;
CREATE INDEX memories_tags_idx ON memories USING GIN (tags);
CREATE INDEX memories_search_trgm_idx ON memories USING GIN (search_text gin_trgm_ops);

CREATE TABLE memory_evidence (
  id UUID PRIMARY KEY,
  memory_id UUID NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
  kind TEXT NOT NULL CHECK (kind IN ('log', 'test', 'link', 'human_note', 'other')),
  label TEXT CHECK (char_length(label) <= 160),
  value TEXT NOT NULL CHECK (char_length(value) BETWEEN 1 AND 2000),
  created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE memory_relations (
  source_memory_id UUID NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
  target_memory_id UUID NOT NULL REFERENCES memories(id) ON DELETE RESTRICT,
  relation_type TEXT NOT NULL CHECK (relation_type IN ('patches', 'contradicts', 'supersedes', 'expires')),
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  PRIMARY KEY (source_memory_id, target_memory_id, relation_type),
  CHECK (source_memory_id <> target_memory_id)
);

CREATE TABLE memory_feedback (
  id UUID PRIMARY KEY,
  memory_id UUID NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
  source_type TEXT NOT NULL CHECK (source_type IN ('agent', 'human')),
  agent_id UUID REFERENCES agents(id) ON DELETE SET NULL,
  developer_id UUID REFERENCES developers(id) ON DELETE SET NULL,
  verdict TEXT NOT NULL CHECK (verdict IN ('useful', 'not_useful', 'worked', 'partially_worked', 'failed')),
  note TEXT CHECK (char_length(note) <= 1200),
  evidence TEXT CHECK (char_length(evidence) <= 2000),
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  CHECK ((source_type = 'agent' AND agent_id IS NOT NULL AND developer_id IS NULL)
      OR (source_type = 'human' AND developer_id IS NOT NULL AND agent_id IS NULL))
);

CREATE INDEX memory_feedback_memory_idx ON memory_feedback (memory_id, source_type);

CREATE TABLE experience_gaps (
  id UUID PRIMARY KEY,
  workspace_id UUID NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
  author_agent_id UUID NOT NULL REFERENCES agents(id) ON DELETE RESTRICT,
  visibility TEXT NOT NULL DEFAULT 'developer_shared' CHECK (visibility IN ('agent_private', 'developer_shared', 'public')),
  question TEXT NOT NULL CHECK (char_length(question) BETWEEN 2 AND 1600),
  context JSONB NOT NULL DEFAULT '{}'::jsonb,
  attempted TEXT CHECK (char_length(attempted) <= 2000),
  language TEXT NOT NULL DEFAULT 'zh-CN' CHECK (char_length(language) BETWEEN 2 AND 20),
  removed_at TIMESTAMPTZ,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX experience_gaps_scope_idx ON experience_gaps (visibility, workspace_id, author_agent_id) WHERE removed_at IS NULL;

CREATE TABLE gap_memory_links (
  gap_id UUID NOT NULL REFERENCES experience_gaps(id) ON DELETE CASCADE,
  memory_id UUID NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  PRIMARY KEY (gap_id, memory_id)
);
