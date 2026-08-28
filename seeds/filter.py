#!/usr/bin/env python3
"""种子数据筛选与质量校验。

功能：
1. 格式校验：必填字段检查、字段类型检查
2. 去重：基于 problem 语义去重（简单规则：相同技术 + 关键词高度重叠）
3. 质量评分：内容长度、conditions 丰富度、action 可操作性等维度
4. 盲测筛选标记：标记为"裸 AI 可能答错"的高价值条目

用法：
    python3 seeds/filter.py --input distilled.json --output filtered.json
    python3 seeds/filter.py --input seeds/seed_memories.json --check-only
"""

import argparse
import hashlib
import json
import re
import sys
from collections import Counter
from pathlib import Path

# 必填字段
REQUIRED_FIELDS = ["problem", "conditions", "action", "outcome", "outcome_kind", "language", "tags"]

# outcome_kind 合法值
VALID_OUTCOME_KINDS = {"success", "partial", "failure", "unknown"}

# conditions 标准 key（来自 STANDARDS.md）
STANDARD_CONDITION_KEYS = {"technologies", "versions", "os", "env", "scale", "language", "team"}

# 低质量关键词（命中则扣分或拒绝）
LOW_QUALITY_PATTERNS = [
    r"^(怎么|如何|怎样|为什么).*$",  # problem 不应该是问句
    r"^(请问|求助|请教|帮忙)",       # 求助语气
    r"^\s*$",                        # 空内容
]


def check_format(item: dict, idx: int = 0) -> tuple[bool, list[str]]:
    """格式校验，返回 (是否通过, 问题列表)。"""
    issues = []

    # 必填字段
    for field in REQUIRED_FIELDS:
        if field not in item:
            issues.append(f"缺少必填字段: {field}")

    # 类型检查
    if "problem" in item and not isinstance(item["problem"], str):
        issues.append("problem 不是字符串")
    if "action" in item and not isinstance(item["action"], str):
        issues.append("action 不是字符串")
    if "outcome" in item and not isinstance(item["outcome"], str):
        issues.append("outcome 不是字符串")
    if "conditions" in item and not isinstance(item["conditions"], dict):
        issues.append("conditions 不是对象")
    if "tags" in item and not isinstance(item["tags"], list):
        issues.append("tags 不是数组")
    if "outcome_kind" in item and item["outcome_kind"] not in VALID_OUTCOME_KINDS:
        issues.append(f"outcome_kind 非法: {item['outcome_kind']}")

    # 内容长度（英文表述更长，上限放宽）
    if "problem" in item and isinstance(item["problem"], str):
        is_en = all(ord(ch) < 128 for ch in item["problem"])
        max_len = 200 if is_en else 120
        if len(item["problem"]) < 5:
            issues.append("problem 太短 (<5字)")
        if len(item["problem"]) > max_len:
            issues.append(f"problem 太长 (>{max_len}字符)")
    if "action" in item and isinstance(item["action"], str):
        if len(item["action"]) < 10:
            issues.append("action 太短，不够具体 (<10字)")
    if "outcome" in item and isinstance(item["outcome"], str):
        if len(item["outcome"]) < 5:
            issues.append("outcome 太短 (<5字)")

    # problem 不应该是问句（中英）
    if "problem" in item and isinstance(item["problem"], str):
        if re.match(r"^(怎么|如何|怎样|为什么|请问)", item["problem"]):
            issues.append("problem 是问句形式，应描述症状")
        if re.match(r"^(how (to|do|can|does)|why (does|is|do|are)|what (is|are|does))\b", item["problem"], re.IGNORECASE):
            issues.append("problem is a question form, should describe the symptom")
        if item["problem"].endswith("?") or item["problem"].endswith("？"):
            issues.append("problem 以问号结尾，应描述症状")

    # conditions 非标准 key 警告
    if "conditions" in item and isinstance(item["conditions"], dict):
        non_standard = set(item["conditions"].keys()) - STANDARD_CONDITION_KEYS
        if non_standard:
            issues.append(f"conditions 含非标 key: {', '.join(non_standard)}（仅警告，不阻断）")

    return len(issues) == 0, issues


def quality_score(item: dict) -> tuple[float, dict]:
    """质量评分 (0-100)，返回 (分数, 各维度详情)。"""
    scores = {}

    # problem 质量 (20分)
    p = item.get("problem", "")
    scores["problem"] = min(20, len(p) * 0.3)  # 越长越详细，但有上限
    symptom_words = ["报错", "失败", "异常", "卡住", "超时", "返回", "无法",
                     "error", "fail", "crash", "timeout", "exception", "cannot", "can't", "not working"]
    if any(t in p.lower() for t in symptom_words):
        scores["problem"] += 5  # 有明确症状词加分
    scores["problem"] = min(20, scores["problem"])

    # conditions 丰富度 (20分)
    cond = item.get("conditions", {})
    if isinstance(cond, dict):
        techs = cond.get("technologies", [])
        versions = cond.get("versions", [])
        scores["conditions"] = len(techs) * 5 + len(versions) * 3
        scores["conditions"] += len([k for k in cond if k not in ("technologies", "versions")]) * 2
        scores["conditions"] = min(20, scores["conditions"])
    else:
        scores["conditions"] = 0

    # action 可操作性 (30分)
    a = item.get("action", "")
    scores["action"] = min(20, len(a) * 0.05)  # 长度基础分
    # 有具体技术细节加分（中英）
    action_keywords = ["修改", "添加", "删除", "配置", "设置", "改为", "使用", "调用", "注册", "创建",
                       "函数", "参数", "配置", "命令", "脚本", "接口", "属性", "方法", "类", "文件",
                       "fix", "set", "add", "update", "remove", "delete", "run", "install", "specify",
                       "command", "file", "config", "flag", "option", "parameter"]
    hits = sum(1 for k in action_keywords if k in a.lower())
    scores["action"] += hits * 1.5
    scores["action"] = min(30, scores["action"])

    # outcome 验证闭环 (15分)
    o = item.get("outcome", "")
    scores["outcome"] = min(10, len(o) * 0.05)
    outcome_keywords = ["解决", "恢复", "正常", "稳定", "通过", "成功", "消失", "下降", "提升", "减少",
                        "resolved", "fixed", "works", "solved", "success"]
    if any(k in o.lower() for k in outcome_keywords):
        scores["outcome"] += 5
    scores["outcome"] = min(15, scores["outcome"])

    # tags 质量 (15分)
    tags = item.get("tags", [])
    if isinstance(tags, list):
        scores["tags"] = min(15, len(tags) * 3)
    else:
        scores["tags"] = 0

    total = sum(scores.values())
    return total, scores


def dedup(items: list[dict]) -> tuple[list[dict], int]:
    """基于 problem + technologies 的去重。"""
    seen = set()
    result = []
    removed = 0

    for item in items:
        problem = item.get("problem", "").strip().lower()
        cond = item.get("conditions", {})
        if isinstance(cond, dict):
            techs = tuple(sorted(cond.get("technologies", [])))
        else:
            techs = tuple()

        # 生成指纹：problem 归一化 + techs
        norm_problem = re.sub(r"[\s，。、；：""''（）()【】\[\]，。！？,.!?]", "", problem)
        # 取前 30 个字符作为指纹基础
        fp_key = (norm_problem[:30], techs)

        if fp_key in seen:
            removed += 1
            continue
        seen.add(fp_key)
        result.append(item)

    return result, removed


def blind_test_flag(item: dict) -> bool:
    """盲测筛选：判断是否属于"裸 AI 容易答错"的高价值条目。

    判据（满足任一即标记）：
    - 涉及具体版本号的兼容性问题
    - 涉及特定环境/操作系统的特有问题
    - 涉及性能调优的具体参数
    - 涉及安全配置的细节陷阱
    - 涉及工具链/构建系统的特殊报错
    """
    text = f"{item.get('problem', '')} {item.get('action', '')} {item.get('outcome', '')}"
    cond = item.get("conditions", {})
    if isinstance(cond, dict):
        versions = cond.get("versions", [])
        os_info = cond.get("os", "")
        env_info = cond.get("env", "")
    else:
        versions = []
        os_info = ""
        env_info = ""

    # 有具体版本号
    if versions and len(versions) > 0:
        return True

    # 有操作系统特定信息
    if os_info and os_info not in ("", "通用"):
        return True

    # 有环境特征
    if env_info:
        return True

    # 命中特定高价值关键词
    high_value_patterns = [
        r"超时|超时时间|连接池|并发|竞态|死锁|内存泄漏|溢出",
        r"权限|认证|鉴权|验签|加密|解密|token|jwt",
        r"编译|构建|链接|打包|部署|启动失败|启动报错",
        r"版本.*不兼容|升级.*报错|降级|回退",
        r"反向代理|负载均衡|网关|dns|域名",
        r"索引|查询优化|执行计划|慢查询|死锁",
    ]
    for pattern in high_value_patterns:
        if re.search(pattern, text):
            return True

    return False


def main() -> None:
    parser = argparse.ArgumentParser(description="种子数据筛选与质量校验")
    parser.add_argument("--input", required=True, help="输入 JSON 文件")
    parser.add_argument("--output", default="filtered.json", help="输出文件路径")
    parser.add_argument("--min-score", type=float, default=50.0, help="最低质量分 (0-100)")
    parser.add_argument("--check-only", action="store_true", help="只检查不输出")
    parser.add_argument("--no-dedup", action="store_true", help="跳过去重")
    args = parser.parse_args()

    in_path = Path(args.input)
    items = json.loads(in_path.read_text(encoding="utf-8"))
    print(f"载入: {len(items)} 条")

    # 1. 格式校验
    print("\n=== 格式校验 ===")
    valid_items = []
    format_errors = 0
    for i, item in enumerate(items):
        ok, issues = check_format(item, i)
        if ok:
            valid_items.append(item)
        else:
            format_errors += 1
            if i < 5:  # 只展示前 5 条错误
                print(f"  [{i}] {item.get('problem', '?')[:40]}...")
                for issue in issues:
                    print(f"      - {issue}")
    print(f"通过: {len(valid_items)}/{len(items)}，失败: {format_errors}")
    if format_errors > 5:
        print(f"  ... 还有 {format_errors - 5} 条格式问题")

    # 2. 去重
    if not args.no_dedup:
        print("\n=== 去重 ===")
        valid_items, removed = dedup(valid_items)
        print(f"去除重复: {removed} 条，剩余: {len(valid_items)} 条")

    # 3. 质量评分
    print("\n=== 质量评分 ===")
    scored = []
    for item in valid_items:
        score, details = quality_score(item)
        item_copy = dict(item)
        item_copy["_quality_score"] = round(score, 1)
        item_copy["_blind_test_value"] = blind_test_flag(item)
        scored.append((score, item_copy))

    scored.sort(key=lambda x: -x[0])

    # 分数分布
    buckets = Counter()
    for score, _ in scored:
        if score >= 80:
            buckets["优秀(80+)"] += 1
        elif score >= 65:
            buckets["良好(65-79)"] += 1
        elif score >= 50:
            buckets["及格(50-64)"] += 1
        else:
            buckets["不及格(<50)"] += 1
    for k in ["优秀(80+)", "良好(65-79)", "及格(50-64)", "不及格(<50)"]:
        print(f"  {k}: {buckets[k]} 条")

    # 盲测高价值统计
    high_value = sum(1 for _, item in scored if item.get("_blind_test_value"))
    print(f"\n盲测高价值条目: {high_value} 条 ({high_value/len(scored)*100:.1f}%)")

    # 过滤低质量
    filtered = [item for _, item in scored if _ >= args.min_score]
    print(f"\n质量分 >= {args.min_score}: {len(filtered)} 条")

    if args.check_only:
        return

    # 清理内部字段后输出
    output = []
    for item in filtered:
        clean = {k: v for k, v in item.items() if not k.startswith("_")}
        output.append(clean)

    out_path = Path(args.output)
    out_path.write_text(json.dumps(output, ensure_ascii=False, indent=2), encoding="utf-8")
    print(f"\n已保存 {len(output)} 条到 {out_path}")

    # 展示 top 5
    print("\n=== 质量分 Top 5 ===")
    for i, (score, item) in enumerate(scored[:5], 1):
        print(f"  {i}. [{score:.1f}] {item['problem'][:60]}")
        techs = item.get("conditions", {}).get("technologies", [])
        if techs:
            print(f"     技术: {', '.join(techs)}")


if __name__ == "__main__":
    main()
