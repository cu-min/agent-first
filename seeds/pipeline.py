#!/usr/bin/env python3
"""种子数据主流水线。

串联：抓取 → 蒸馏 → 筛选 → 去重 → 合并 → 导入

用法：
    # 完整流水线（需要 LLM API）
    python3 seeds/pipeline.py --target 300 --server http://localhost:8080

    # 只用本地已有数据合并 + 去重 + 导入
    python3 seeds/pipeline.py --skip-fetch --skip-distill --server http://localhost:8080

    # 只生成最终 JSON，不导入
    python3 seeds/pipeline.py --skip-import
"""

import argparse
import json
import subprocess
import sys
import tempfile
from pathlib import Path

SEEDS_DIR = Path(__file__).parent
SEED_OUTPUT = SEEDS_DIR / "seed_memories.json"


def run_cmd(cmd: list[str], cwd: Path | None = None) -> bool:
    """运行命令，返回是否成功。"""
    print(f"\n>>> 执行: {' '.join(cmd)}")
    try:
        result = subprocess.run(cmd, cwd=str(cwd) if cwd else None, capture_output=False)
        return result.returncode == 0
    except FileNotFoundError as e:
        print(f"  命令不存在: {e}", file=sys.stderr)
        return False


def merge_json_files(inputs: list[Path], output: Path) -> int:
    """合并多个 JSON 数组文件。"""
    all_items = []
    for f in inputs:
        if f.exists():
            items = json.loads(f.read_text(encoding="utf-8"))
            print(f"  读取 {f.name}: {len(items)} 条")
            all_items.extend(items)
    output.write_text(json.dumps(all_items, ensure_ascii=False, indent=2), encoding="utf-8")
    print(f"  合并完成，共 {len(all_items)} 条 → {output.name}")
    return len(all_items)


def main() -> None:
    parser = argparse.ArgumentParser(description="种子数据主流水线")
    parser.add_argument("--target", type=int, default=300, help="目标条数")
    parser.add_argument("--server", default=None, help="服务端地址（用于导入）")
    parser.add_argument("--skip-fetch", action="store_true", help="跳过抓取步骤")
    parser.add_argument("--skip-distill", action="store_true", help="跳过蒸馏步骤")
    parser.add_argument("--skip-filter", action="store_true", help="跳过筛选步骤")
    parser.add_argument("--skip-import", action="store_true", help="跳过导入步骤")
    parser.add_argument("--so-tags", default="rust,reactjs,postgresql,spring-boot,docker,node.js,python",
                        help="Stack Overflow 抓取标签")
    parser.add_argument("--so-pages", type=int, default=5, help="SO 抓取页数")
    parser.add_argument("--gh-repos", default="tokio-rs/tokio,facebook/react,spring-projects/spring-boot",
                        help="GitHub 抓取仓库")
    parser.add_argument("--gh-pages", type=int, default=2, help="GitHub 每仓库页数")
    parser.add_argument("--distill-mode", choices=["builtin", "llm"], default="builtin",
                        help="蒸馏模式（builtin=对话内蒸馏，llm=API 调用，仅作后备）")
    args = parser.parse_args()

    work_dir = Path(tempfile.mkdtemp(prefix="seed_pipeline_"))
    print(f"工作目录: {work_dir}")

    so_raw = work_dir / "so_raw.json"
    gh_raw = work_dir / "gh_raw.json"
    so_distilled = work_dir / "so_distilled.json"
    gh_distilled = work_dir / "gh_distilled.json"
    merged_raw = work_dir / "merged_raw.json"
    filtered = work_dir / "filtered.json"

    # 1. 抓取
    if not args.skip_fetch:
        print("\n" + "=" * 60)
        print("步骤 1/5: 抓取原始数据")
        print("=" * 60)

        so_ok = run_cmd([
            sys.executable, str(SEEDS_DIR / "fetch_so.py"),
            "--tags", args.so_tags,
            "--pages", str(args.so_pages),
            "--output", str(so_raw),
        ])
        if not so_ok:
            print("  SO 抓取失败，继续...")

        gh_ok = run_cmd([
            sys.executable, str(SEEDS_DIR / "fetch_github.py"),
            "--repos", args.gh_repos,
            "--pages", str(args.gh_pages),
            "--output", str(gh_raw),
        ])
        if not gh_ok:
            print("  GitHub 抓取失败，继续...")
    else:
        print("\n[跳过] 抓取步骤")

    # 2. 蒸馏
    if not args.skip_distill:
        print("\n" + "=" * 60)
        print("步骤 2/5: 结构化蒸馏")
        print("=" * 60)

        if so_raw.exists():
            run_cmd([
                sys.executable, str(SEEDS_DIR / "distill.py"),
                "--input", str(so_raw),
                "--output", str(so_distilled),
                "--mode", args.distill_mode,
            ])
        if gh_raw.exists():
            run_cmd([
                sys.executable, str(SEEDS_DIR / "distill.py"),
                "--input", str(gh_raw),
                "--output", str(gh_distilled),
                "--mode", args.distill_mode,
            ])
    else:
        print("\n[跳过] 蒸馏步骤")

    # 3. 合并
    print("\n" + "=" * 60)
    print("步骤 3/5: 合并数据")
    print("=" * 60)

    inputs = []
    if so_distilled.exists():
        inputs.append(so_distilled)
    if gh_distilled.exists():
        inputs.append(gh_distilled)
    # 也包含现有的种子数据
    if SEED_OUTPUT.exists():
        inputs.append(SEED_OUTPUT)

    if not inputs:
        print("  没有可合并的数据！")
        sys.exit(1)

    total = merge_json_files(inputs, merged_raw)
    print(f"  合并后: {total} 条")

    # 4. 筛选 + 去重
    if not args.skip_filter:
        print("\n" + "=" * 60)
        print("步骤 4/5: 质量筛选 + 去重")
        print("=" * 60)

        run_cmd([
            sys.executable, str(SEEDS_DIR / "filter.py"),
            "--input", str(merged_raw),
            "--output", str(filtered),
            "--min-score", "40",
        ])
    else:
        filtered = merged_raw
        print("\n[跳过] 筛选步骤")

    # 5. 更新种子文件 + 导入
    print("\n" + "=" * 60)
    print("步骤 5/5: 更新种子文件" + (" + 导入" if not args.skip_import and args.server else ""))
    print("=" * 60)

    final_items = json.loads(filtered.read_text(encoding="utf-8"))

    # 如果数量不够目标，保留所有
    if len(final_items) >= args.target:
        final_items = final_items[:args.target]
        print(f"  截取前 {args.target} 条")
    else:
        print(f"  最终 {len(final_items)} 条（目标 {args.target}，差 {args.target - len(final_items)} 条）")

    SEED_OUTPUT.write_text(json.dumps(final_items, ensure_ascii=False, indent=2), encoding="utf-8")
    print(f"  已更新 {SEED_OUTPUT}")

    # 质量报告
    run_cmd([
        sys.executable, str(SEEDS_DIR / "filter.py"),
        "--input", str(SEED_OUTPUT),
        "--check-only",
    ])

    if not args.skip_import and args.server:
        print("\n导入到服务端...")
        run_cmd([
            sys.executable, str(SEEDS_DIR / "import_seeds.py"),
            args.server,
        ])

    print(f"\n流水线完成！最终种子文件: {SEED_OUTPUT}")
    print(f"条目数: {len(final_items)}")


if __name__ == "__main__":
    main()
