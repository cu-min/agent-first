#!/usr/bin/env python3
"""Stack Overflow 双进料抓取器（分层语料 v2）。

两种进料（STANDARDS.md §5.6）：
    common  垫底层：高赞（score>=60）或 高频迭代反馈（score>=20 且回答数>=5），有采纳答案
    core    核心层：冷门已解决（0<=score<=5），有采纳答案 —— 盲测主战场

两路进料都过栈白名单过滤（tags 命中白名单才保留）。

用法：
    python3 seeds/fetch_so.py --feed common --pages 3 --output so_raw_common.json
    python3 seeds/fetch_so.py --feed core --pages 3 --output so_raw_core.json
"""

import argparse
import json
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path

SO_API_BASE = "https://api.stackexchange.com/2.3"
FILTER = "withbody"  # 官方稳定 filter：default + 所有 body 字段（question/answer 正文）

# 栈白名单：问题 tags 与此集合有交集才保留（剔除蹭标签/离栈内容）
STACK_WHITELIST = {
    "rust", "cargo", "tokio", "axum", "sqlx", "async-await", "async-rust",
    "reactjs", "react-hooks", "typescript", "vite", "javascript",
    "postgresql", "pgvector", "sql", "redis",
    "docker", "docker-compose", "kubernetes", "nginx",
    "node.js", "npm",
    "python", "pip",
    "spring-boot", "java",
    "git", "github-actions", "webpack",
}

# common 层双路条件（OR 关系，两路抓取后按 question_id 合并去重）
# 注意：sort 必须显式带 votes，min/max 才作用于分数
COMMON_FEEDS = [
    {"sort": "votes", "order": "desc", "min": 60, "answers": 1},   # 高赞
    {"sort": "votes", "order": "desc", "min": 20, "answers": 5},   # 高频迭代反馈
]
# core 层：冷门已解决长尾
CORE_FEEDS = [
    {"sort": "votes", "order": "asc", "min": 0, "max": 5, "answers": 1},
]

DEFAULT_TAGS = "rust,reactjs,postgresql,spring-boot,docker,node.js,python,typescript,kubernetes"
# SO API 实测：tagged 分号 OR 最多 2 个 tag，超过静默返回空（2026-08-29 调试结论）
TAG_GROUP_SIZE = 2


def fetch_questions(tags: list[str], page: int, pagesize: int, feed_params: dict) -> list[dict]:
    """按进料条件抓取一页已解决问题。"""
    tag_str = ";".join(tags)
    params = {
        "site": "stackoverflow",
        "filter": FILTER,
        "tagged": tag_str,
        "accepted": "True",
        "pagesize": pagesize,
        "page": page,
    }
    params.update(feed_params)
    url = f"{SO_API_BASE}/questions?{urllib.parse.urlencode(params)}"
    req = urllib.request.Request(url, headers={"User-Agent": "agent-first-seed-importer/1.0"})
    try:
        with urllib.request.urlopen(req, timeout=30) as resp:
            data = json.loads(resp.read())
            return data.get("items", [])
    except urllib.error.HTTPError as e:
        if e.code == 429:
            backoff = int(e.headers.get("X-Error-Detail", "10").split()[-1])
            print(f"  限流了，等待 {backoff}s...", file=sys.stderr)
            time.sleep(backoff)
            return fetch_questions(tags, page, pagesize, feed_params)
        print(f"  SO API 错误 {e.code}: {e.read().decode()}", file=sys.stderr)
        return []


def fetch_answers_batch(question_ids: list[int]) -> dict[int, list[dict]]:
    """批量抓取多个问题的回答（每批 50 个 id），按 question_id 分组返回。

    注意：answers API 的 pagesize 限制的是单页返回的 answer 总数，
    不是每题上限，所以必须翻页拉全（has_more）。
    """
    result: dict[int, list[dict]] = {}
    for i in range(0, len(question_ids), 50):
        batch = question_ids[i:i + 50]
        ids_str = ";".join(str(qid) for qid in batch)
        page = 1
        while page <= 5:  # 最多翻 5 页防失控
            params = urllib.parse.urlencode({
                "order": "desc",
                "sort": "votes",
                "site": "stackoverflow",
                "filter": FILTER,
                "pagesize": 100,
                "page": page,
            })
            url = f"{SO_API_BASE}/questions/{ids_str}/answers?{params}"
            req = urllib.request.Request(url, headers={"User-Agent": "agent-first-seed-importer/1.0"})
            try:
                with urllib.request.urlopen(req, timeout=30) as resp:
                    data = json.loads(resp.read())
            except urllib.error.HTTPError as e:
                print(f"  SO answers API 错误 {e.code}", file=sys.stderr)
                break
            items = data.get("items", [])
            for a in items:
                qid = a.get("question_id")
                result.setdefault(qid, []).append(a)
            if not data.get("has_more"):
                break
            page += 1
            time.sleep(0.3)
        time.sleep(0.3)
    return result


def extract_items(raw_questions: list[dict]) -> list[dict]:
    """把 SO 原始响应转为内部结构。"""
    result = []
    for q in raw_questions:
        answers = []
        for a in q.get("answers", []):
            answers.append({
                "answer_id": a["answer_id"],
                "body": a.get("body", ""),
                "score": a.get("score", 0),
                "is_accepted": a.get("is_accepted", False),
                "owner_display_name": a.get("owner", {}).get("display_name", ""),
            })
        answers.sort(key=lambda x: (-x["is_accepted"], -x["score"]))
        result.append({
            "source": "stackoverflow",
            "question_id": q["question_id"],
            "title": q.get("title", ""),
            "body": q.get("body", ""),
            "tags": q.get("tags", []),
            "score": q.get("score", 0),
            "answer_count": q.get("answer_count", 0),
            "accepted_answer_id": q.get("accepted_answer_id"),
            "answers": answers,
            "link": q.get("link", ""),
            "owner_display_name": q.get("owner", {}).get("display_name", ""),
            "creation_date": q.get("creation_date"),
        })
    return result


def stack_ok(tags: list[str]) -> bool:
    """栈白名单过滤：tags 与白名单有交集。"""
    return bool({t.lower() for t in tags} & STACK_WHITELIST)


def main() -> None:
    parser = argparse.ArgumentParser(description="抓取 Stack Overflow 已解决问题（分层双进料）")
    parser.add_argument("--feed", choices=["common", "core"], default="common",
                        help="进料层：common=高赞/高频反馈垫底层，core=冷门已解决核心层")
    parser.add_argument("--tags", default=DEFAULT_TAGS, help="标签列表，逗号分隔")
    parser.add_argument("--pages", type=int, default=3, help="每路抓取页数（每页约 20 条）")
    parser.add_argument("--pagesize", type=int, default=20, help="每页条数")
    parser.add_argument("--output", default=None, help="输出文件路径（默认 so_raw_{feed}.json）")
    args = parser.parse_args()

    tags = [t.strip() for t in args.tags.split(",") if t.strip()]
    feeds = COMMON_FEEDS if args.feed == "common" else CORE_FEEDS
    out_path = Path(args.output or f"so_raw_{args.feed}.json")

    # SO API 限制：tagged 分号 OR 最多 2 个 tag，自动两两分组多轮抓取
    tag_groups = [tags[i:i + TAG_GROUP_SIZE] for i in range(0, len(tags), TAG_GROUP_SIZE)]

    print(f"进料层: {args.feed} | 抓取标签: {tags}（{len(tag_groups)} 组）")
    print(f"进料条件: {feeds} | 每路 {args.pages} 页 × {args.pagesize} 条")

    all_items: list[dict] = []
    seen_ids: set[int] = set()
    dropped_stack = 0

    for feed_idx, feed_params in enumerate(feeds, 1):
        feed_desc = f"score>={feed_params.get('min', 0)}"
        if "max" in feed_params:
            feed_desc += f" 且 score<={feed_params['max']}"
        if feed_params.get("answers", 1) > 1:
            feed_desc += f" 且 answers>={feed_params['answers']}"
        print(f"\n第 {feed_idx}/{len(feeds)} 路（{feed_desc}）:")
        for group in tag_groups:
            print(f"  标签组 {group}:")
            for page in range(1, args.pages + 1):
                print(f"    第 {page}/{args.pages} 页...", end=" ", flush=True)
                raw = fetch_questions(group, page, args.pagesize, feed_params)
                items = extract_items(raw)
                new_items = [it for it in items if it["question_id"] not in seen_ids]
                for it in new_items:
                    seen_ids.add(it["question_id"])
                kept = [it for it in new_items if stack_ok(it["tags"])]
                dropped_stack += len(new_items) - len(kept)
                all_items.extend(kept)
                print(f"新增 {len(new_items)}，栈过滤后保留 {len(kept)}（累计 {len(all_items)}）")
                time.sleep(0.5)  # 礼貌限速

    # 所有问题抓完，批量拉回答
    if all_items:
        print(f"\n批量拉取 {len(all_items)} 个问题的回答...")
        answers_map = fetch_answers_batch([it["question_id"] for it in all_items])
        for it in all_items:
            raw_answers = answers_map.get(it["question_id"], [])
            answers = [
                {
                    "answer_id": a["answer_id"],
                    "body": a.get("body", ""),
                    "score": a.get("score", 0),
                    "is_accepted": a.get("is_accepted", False),
                    "owner_display_name": a.get("owner", {}).get("display_name", ""),
                }
                for a in raw_answers
            ]
            answers.sort(key=lambda x: (-x["is_accepted"], -x["score"]))
            it["answers"] = answers
        has_answer = sum(1 for it in all_items if it["answers"])
        print(f"含回答: {has_answer}/{len(all_items)}")

    # 统计
    print(f"\n=== {args.feed} 进料统计 ===")
    print(f"  总条数: {len(all_items)} | 栈白名单剔除: {dropped_stack}")
    if all_items:
        scores = sorted(it["score"] for it in all_items)
        ans_counts = sorted(it["answer_count"] for it in all_items)
        print(f"  score: min={scores[0]} 中位={scores[len(scores) // 2]} max={scores[-1]}")
        print(f"  回答数: min={ans_counts[0]} 中位={ans_counts[len(ans_counts) // 2]} max={ans_counts[-1]}")
        tag_counter: dict[str, int] = {}
        for it in all_items:
            for t in it["tags"]:
                tag_counter[t.lower()] = tag_counter.get(t.lower(), 0) + 1
        top_tags = sorted(tag_counter.items(), key=lambda x: -x[1])[:10]
        print(f"  高频标签: {', '.join(f'{t}({c})' for t, c in top_tags)}")

    out_path.write_text(json.dumps(all_items, ensure_ascii=False, indent=2), encoding="utf-8")
    print(f"\n完成，共 {len(all_items)} 条，已保存到 {out_path}")


if __name__ == "__main__":
    main()
