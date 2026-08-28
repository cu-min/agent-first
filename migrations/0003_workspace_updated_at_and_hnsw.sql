ALTER TABLE workspaces ADD COLUMN updated_at TIMESTAMPTZ NOT NULL DEFAULT now();

CREATE INDEX memories_embedding_hnsw_idx ON memories USING hnsw (embedding vector_cosine_ops);
