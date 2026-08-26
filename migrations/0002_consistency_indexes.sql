-- 上线前一致性收口：锁定 embedding 维度、补检索与清理所需索引
-- 依赖 pgvector >= 0.5（支持对已有无类型列执行 ALTER TYPE vector(n)，逐行校验存量数据）

ALTER TABLE memories ALTER COLUMN embedding TYPE vector(1024);

-- 检索条件 technologies 过滤（conditions->'technologies' ? $3）走 GIN
CREATE INDEX memories_conditions_technologies_idx ON memories USING GIN ((conditions->'technologies'));

-- 移除记忆时按 target_memory_id 清理 relations
CREATE INDEX memory_relations_target_idx ON memory_relations (target_memory_id);

-- 移除记忆时清理 gap_memory_links
CREATE INDEX gap_memory_links_memory_idx ON gap_memory_links (memory_id);
