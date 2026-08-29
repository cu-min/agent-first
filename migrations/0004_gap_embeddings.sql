-- 缺口语义检索：与 memories 同规格的 vector(1024) 列与 HNSW 索引
ALTER TABLE experience_gaps ADD COLUMN IF NOT EXISTS embedding vector(1024);
CREATE INDEX IF NOT EXISTS experience_gaps_embedding_hnsw_idx ON experience_gaps USING hnsw (embedding vector_cosine_ops);
