#!/usr/bin/env python3
"""蒸馏后同题去重预检（直连 pgvector，只读）。

对每条候选记忆：算 embedding → 与库内已发布记忆算余弦相似度 → top1 过高判同题。
防止同题条目挤占检索前排（详见 PROJECT_MEMORY 踩坑记录）。

用法：
    # 报告模式：打印每条的 top1 相似度分布
    python3 seeds/dedup_check.py --input distilled.json

    # 剪除模式：剔除同题后输出干净版
    python3 seeds/dedup_check.py --input distilled.json --prune --output clean.json
"""

import argparse
import json
import time
import urllib.parse
import urllib.request
from pathlib import Path

import psycopg2

DB = dict(host="127.0.0.1", port=5433, dbname="agentfirst",
          user="agentfirst", password="agentfirst_dev_only")
EMBED_BATCH = 16  # 智谱单批条数上限内，保守取 16


def load_env() -> dict:
    env = {}
    env_path = Path(__file__).parent.parent / ".env"
    with open(env_path, encoding="utf-8") as f:
        for line in f:
            line = line.strip()
            if line and not line.startswith("#") and "=" in line:
                k, v = line.split("=", 1)
                env[k.strip()] = v.strip()
    return env


def embed(texts: list[str], env: dict) -> list[list[float]]:
    """智谱 Embedding-3，1024 维（与库内向量对齐，dimensions 必传）。"""
    out = []
    for i in range(0, len(texts), EMBED_BATCH):
        batch = texts[i:i + EMBED_BATCH]
        req = urllib.request.Request(
            env["EMBEDDING_ENDPOINT"],
            data=json.dumps({"model": env["EMBEDDING_MODEL"], "input": batch,
                             "dimensions": 1024}).encode("utf-8"),
            headers={"Authorization": "Bearer " + env["EMBEDDING_API_KEY"],
                     "Content-Type": "application/json"},
            method="POST",
        )
        with urllib.request.urlopen(req, timeout=60) as r:
            d = json.load(r)
        order = {item["index"]: item["embedding"] for item in d["data"]}
        out.extend(order[i] for i in range(len(batch)))
        time.sleep(0.2)
    return out


def top1_similarity(cur, vec: list[float]) -> tuple[float, str]:
    """对库内已发布记忆算 top1 余弦相似度。"""
    vec_str = "[" + ",".join(f"{x:.6f}" for x in vec) + "]"
    cur.execute("""
        SELECT problem, 1 - (embedding <=> %s::vector) AS sim
        FROM memories
        WHERE removed_at IS NULL AND published_at IS NOT NULL
        ORDER BY embedding <=> %s::vector
        LIMIT 1
    """, (vec_str, vec_str))
    row = cur.fetchone()
    return (row[1], row[0]) if row else (0.0, "")


def main() -> None:
    parser = argparse.ArgumentParser(description="同题去重预检")
    parser.add_argument("--input", required=True, help="蒸馏后的 JSON 文件")
    parser.add_argument("--prune", action="store_true", help="剔除同题后输出干净版")
    parser.add_argument("--output", default=None, help="剪除模式输出路径")
    parser.add_argument("--threshold", type=float, default=0.82, help="同题判定阈值")
    args = parser.parse_args()

    items = json.loads(Path(args.input).read_text(encoding="utf-8"))
    print(f"候选 {len(items)} 条 | 同题阈值 {args.threshold}")

    env = load_env()
    conn = psycopg2.connect(**DB)
    cur = conn.cursor()

    texts = [it["problem"] for it in items]
    print("计算 embedding...")
    vectors = embed(texts, env)

    rows = []
    for i, (it, vec) in enumerate(zip(items, vectors)):
        sim, matched = top1_similarity(cur, vec)
        rows.append({"idx": i, "sim": sim, "matched": matched,
                     "problem": it["problem"][:60]})
        if (i + 1) % 20 == 0:
            print(f"  进度 {i + 1}/{len(items)}")

    conn.close()

    sims = sorted(r["sim"] for r in rows)
    print(f"\n=== top1 相似度分布 ===")
    print(f"  min={sims[0]:.3f} 中位={sims[len(sims) // 2]:.3f} max={sims[-1]:.3f}")
    buckets = [(0, 0.5), (0.5, 0.7), (0.7, 0.82), (0.82, 0.9), (0.9, 1.01)]
    for lo, hi in buckets:
        n = sum(1 for s in sims if lo <= s < hi)
        print(f"  [{lo:.2f}, {hi:.2f}): {n} 条")

    dups = [r for r in rows if r["sim"] >= args.threshold]
    print(f"\n同题（top1 >= {args.threshold}）: {len(dups)} 条")
    for r in dups:
        print(f"  {r['sim']:.3f} | {r['problem']}")
        print(f"        库内: {r['matched'][:56]}")

    if args.prune:
        dup_idx = {r["idx"] for r in dups}
        clean = [it for i, it in enumerate(items) if i not in dup_idx]
        out_path = Path(args.output or args.input.replace(".json", "_dedup.json"))
        out_path.write_text(json.dumps(clean, ensure_ascii=False, indent=2), encoding="utf-8")
        print(f"\n剪除模式：剔除 {len(dup_idx)} 条，保留 {len(clean)} 条 → {out_path}")


if __name__ == "__main__":
    main()
