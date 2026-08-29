#!/usr/bin/env python3
"""从原料池取一批生成紧凑视图（供对话内蒸馏），剥 HTML、截断超长。

用法：
    python3 seeds/take_batch.py --pool common --start 0 --count 14
"""
import argparse
import html
import json
import re
from pathlib import Path

SEEDS = Path(__file__).parent


def strip_html(text: str) -> str:
    text = re.sub(r"<(script|style)[^>]*>.*?</\1>", " ", text, flags=re.S | re.I)
    text = re.sub(r"<br\s*/?>", "\n", text, flags=re.I)
    text = re.sub(r"</(p|div|li|pre|blockquote|h[1-6]|tr)>", "\n", text, flags=re.I)
    text = re.sub(r"<li[^>]*>", "  - ", text, flags=re.I)
    text = re.sub(r"<code[^>]*>", "`", text, flags=re.I)
    text = re.sub(r"</code>", "`", text, flags=re.I)
    text = re.sub(r"<[^>]+>", "", text)
    text = html.unescape(text)
    text = re.sub(r"\n{3,}", "\n\n", text)
    return text.strip()


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--pool", choices=["common", "core"], required=True)
    parser.add_argument("--pool-file", default=None, help="自定义池文件路径（覆盖 --pool 的默认池）")
    parser.add_argument("--start", type=int, default=0)
    parser.add_argument("--count", type=int, default=14)
    parser.add_argument("--max-body", type=int, default=1400, help="问题正文截断长度")
    parser.add_argument("--max-answer", type=int, default=2000, help="回答正文截断长度")
    args = parser.parse_args()

    pool_path = Path(args.pool_file) if args.pool_file else SEEDS / f"_so_{args.pool}_pool.json"
    items = json.loads(pool_path.read_text(encoding="utf-8"))
    batch = items[args.start:args.start + args.count]
    if not batch:
        print("没有更多条目")
        return

    out = []
    for it in batch:
        acc = next((a for a in it["answers"] if a["is_accepted"]), None) or (
            it["answers"][0] if it["answers"] else None)
        entry = {
            "n": len(out) + 1,
            "qid": it["question_id"],
            "title": it["title"],
            "tags": it["tags"],
            "score": it["score"],
            "answers_n": it["answer_count"],
            "problem_src": strip_html(it["body"])[:args.max_body],
            "answer_src": strip_html(acc["body"])[:args.max_answer] if acc else "",
            "answer_by": acc.get("owner_display_name", "") if acc else "",
            "asked_by": it.get("owner_display_name", ""),
            "link": it["link"],
        }
        out.append(entry)

    pool_name = pool_path.stem.replace("_so_", "").replace("_pool", "")
    view_path = SEEDS / f"_view_{pool_name}_{args.start + 1:03d}_{args.start + len(batch):03d}.json"
    view_path.write_text(json.dumps(out, ensure_ascii=False, indent=1), encoding="utf-8")
    print(f"批次 {args.start + 1}-{args.start + len(batch)} / 共 {len(items)} 条 → {view_path.name}")
    for e in out:
        print(f"  {e['n']:2}. [{e['score']:4}分/{e['answers_n']}答] {e['title'][:70]}")
    print(f"\n剩余 {len(items) - args.start - len(batch)} 条")


if __name__ == "__main__":
    main()
