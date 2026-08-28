#!/usr/bin/env python3
"""Stack Overflow 已解决问题抓取器。

用法：
    python3 seeds/fetch_so.py --tags rust,react,postgresql --pages 5 --output so_raw.json

输出 JSON 结构：
[
  {
    "source": "stackoverflow",
    "question_id": 123456,
    "title": "...",
    "body": "...",
    "tags": ["rust", "axum"],
    "score": 42,
    "answer_count": 3,
    "accepted_answer_id": 789012,
    "answers": [
      {"answer_id": 789012, "body": "...", "score": 38, "is_accepted": true},
      ...
    ],
    "link": "https://stackoverflow.com/questions/123456/...",
    "owner_display_name": "username"
  },
  ...
]
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


def fetch_questions(tags: list[str], page: int, pagesize: int = 20) -> list[dict]:
    """抓取一页已解决问题（有 accepted answer）。"""
    tag_str = ";".join(tags)
    params = {
        "order": "desc",
        "sort": "votes",
        "site": "stackoverflow",
        "filter": FILTER,
        "tagged": tag_str,
        "accepted": "True",
        "answers": 5,
        "pagesize": pagesize,
        "page": page,
    }
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
            return fetch_questions(tags, page, pagesize)
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


def main() -> None:
    parser = argparse.ArgumentParser(description="抓取 Stack Overflow 已解决问题")
    parser.add_argument("--tags", default="rust,reactjs,postgresql,spring-boot,docker,node.js,python",
                        help="标签列表，逗号分隔")
    parser.add_argument("--pages", type=int, default=5, help="抓取页数（每页约 20 条）")
    parser.add_argument("--pagesize", type=int, default=20, help="每页条数")
    parser.add_argument("--output", default="so_raw.json", help="输出文件路径")
    args = parser.parse_args()

    tags = [t.strip() for t in args.tags.split(",") if t.strip()]
    all_items: list[dict] = []
    seen_ids: set[int] = set()

    print(f"抓取标签: {tags}")
    print(f"计划抓取 {args.pages} 页，每页 {args.pagesize} 条")

    for page in range(1, args.pages + 1):
        print(f"  第 {page}/{args.pages} 页...", end=" ", flush=True)
        raw = fetch_questions(tags, page, args.pagesize)
        items = extract_items(raw)
        new_items = [it for it in items if it["question_id"] not in seen_ids]
        for it in new_items:
            seen_ids.add(it["question_id"])
        all_items.extend(new_items)
        print(f"新增 {len(new_items)} 条（累计 {len(all_items)}）")
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
                }
                for a in raw_answers
            ]
            answers.sort(key=lambda x: (-x["is_accepted"], -x["score"]))
            it["answers"] = answers
        has_answer = sum(1 for it in all_items if it["answers"])
        print(f"含回答: {has_answer}/{len(all_items)}")

    out_path = Path(args.output)
    out_path.write_text(json.dumps(all_items, ensure_ascii=False, indent=2), encoding="utf-8")
    print(f"\n完成，共 {len(all_items)} 条，已保存到 {out_path}")


if __name__ == "__main__":
    main()
