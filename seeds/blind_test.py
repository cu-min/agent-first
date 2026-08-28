#!/usr/bin/env python3
"""对话模型盲测工具（不调 API）。

盲测模型 = 当前对话模型，由其在对话中亲自作答。理由：
记忆库的实际消费者就是这类 agent 模型，用它当尺子测最准——
它已经会的条目对它没有增量价值，直接丢弃。

协议保证真盲（顺序不可颠倒）：
  阶段1 sheet   只输出 问题+环境（屏蔽真实解法），对话模型作答
  阶段2 answers 对话模型把答案写入 _blind_answers.json（此阶段严禁查看 action）
  阶段3 judge   生成"我的答案 vs 真实解法"对照，对话模型判定等价性
  阶段4 verdicts 对话模型写入 _blind_verdicts.json
  阶段5 apply   equivalent=true（模型已会）丢弃，false（高价值）保留

用法：
    python blind_test.py sheet  --input distilled.json [--start 1] [--count 50]
    python blind_test.py judge  --input distilled.json [--start 1] [--count 50]
    python blind_test.py apply  --input distilled.json --output blind_filtered.json
"""

import argparse
import json
import sys
from pathlib import Path

SHEET = Path("_blind_sheet.json")
ANSWERS = Path("_blind_answers.json")
PAIRS = Path("_blind_pairs.json")
VERDICTS = Path("_blind_verdicts.json")


def load_json(p: Path):
    if not p.exists():
        print(f"错误：找不到 {p}，请先完成上一步", file=sys.stderr)
        sys.exit(1)
    return json.loads(p.read_text(encoding="utf-8"))


def save_json(p: Path, data) -> None:
    p.write_text(json.dumps(data, ensure_ascii=False, indent=2), encoding="utf-8")


def select_range(items: list, start: int, count: int) -> list:
    end = len(items) if not count else min(len(items), start - 1 + count)
    return items[start - 1:end], end


def cond_str(cond: dict) -> str:
    parts = []
    if cond.get("technologies"):
        parts.append(",".join(cond["technologies"]))
    for k in ("versions", "os", "env", "scale"):
        if cond.get(k):
            v = cond[k]
            parts.append(f"{k}={','.join(v) if isinstance(v, list) else v}")
    return " | ".join(parts)


def cmd_sheet(args) -> None:
    items = load_json(Path(args.input))
    sel, end = select_range(items, args.start, args.count)
    sheet = [
        {"id": i, "problem": it.get("problem", ""), "conditions": it.get("conditions", {})}
        for i, it in zip(range(args.start, end + 1), sel)
    ]
    save_json(SHEET, sheet)
    print(f"已生成 {SHEET}（{len(sheet)} 条，id {args.start}~{end}）\n")
    print("=" * 76)
    for s in sheet:
        print(f"[{s['id']}] {s['problem']}")
        c = cond_str(s["conditions"])
        if c:
            print(f"     环境: {c}")
    print("=" * 76)
    print("\n>> 请对话模型逐条作答（未看真实解法），答案写入 _blind_answers.json")
    print(">> 格式: [{\"id\": 1, \"answer\": \"...\"}, ...]")


def cmd_judge(args) -> None:
    items = load_json(Path(args.input))
    answers = {a["id"]: a["answer"] for a in load_json(ANSWERS)}
    sel, end = select_range(items, args.start, args.count)

    pairs = []
    missing = []
    for i, it in zip(range(args.start, end + 1), sel):
        if i not in answers:
            missing.append(i)
            continue
        pairs.append({
            "id": i,
            "problem": it.get("problem", ""),
            "my_answer": answers[i],
            "real_action": it.get("action", ""),
            "outcome": it.get("outcome", ""),
        })
    if missing:
        print(f"警告：{len(missing)} 条无答案被跳过: {missing}", file=sys.stderr)
    save_json(PAIRS, pairs)

    print(f"已生成 {PAIRS}（{len(pairs)} 对）\n")
    for p in pairs:
        print(f"[{p['id']}] {p['problem']}")
        print(f"  我的答案: {p['my_answer']}")
        print(f"  真实解法: {p['real_action']}")
        print(f"  原帖结果: {p['outcome']}")
        print()
    print(">> 请对话模型逐条判定：我的答案与真实解法是否实质等价")
    print(">> 等价 = 同一根因 + 同一修复方向（关键技术点一致）")
    print(">> 写入 _blind_verdicts.json，格式: [{\"id\": 1, \"equivalent\": true}, ...]")


def cmd_apply(args) -> None:
    items = load_json(Path(args.input))
    verdicts = {v["id"]: v["equivalent"] for v in load_json(VERDICTS)}

    keep, drop_ids, untested = [], [], []
    for i, it in enumerate(items, 1):
        if i not in verdicts:
            untested.append(i)
            keep.append(it)
        elif verdicts[i]:
            drop_ids.append(i)
        else:
            keep.append(it)

    out = Path(args.output)
    save_json(out, keep)
    print(f"总输入: {len(items)} 条")
    print(f"  已会丢弃: {len(drop_ids)} 条")
    print(f"  高价值保留: {len(keep) - len(untested)} 条")
    if untested:
        print(f"  ⚠ 未完成盲测直接保留: {len(untested)} 条 (id: {untested[:20]}{'...' if len(untested) > 20 else ''})")
    print(f"\n已保存 {len(keep)} 条到 {out}")


def main() -> None:
    parser = argparse.ArgumentParser(description="对话模型盲测工具（不调 API）")
    sub = parser.add_subparsers(dest="cmd", required=True)

    p_sheet = sub.add_parser("sheet", help="阶段1：输出题目清单（屏蔽真实解法）")
    p_sheet.add_argument("--input", required=True)
    p_sheet.add_argument("--start", type=int, default=1)
    p_sheet.add_argument("--count", type=int, default=0, help="本次条数（0=从 start 到结尾）")
    p_sheet.set_defaults(func=cmd_sheet)

    p_judge = sub.add_parser("judge", help="阶段3：生成我的答案 vs 真实解法对照")
    p_judge.add_argument("--input", required=True)
    p_judge.add_argument("--start", type=int, default=1)
    p_judge.add_argument("--count", type=int, default=0)
    p_judge.set_defaults(func=cmd_judge)

    p_apply = sub.add_parser("apply", help="阶段5：应用判定，输出保留条目")
    p_apply.add_argument("--input", required=True)
    p_apply.add_argument("--output", required=True)
    p_apply.set_defaults(func=cmd_apply)

    args = parser.parse_args()
    args.func(args)


if __name__ == "__main__":
    main()
