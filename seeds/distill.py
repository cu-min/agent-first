#!/usr/bin/env python3
"""结构化蒸馏：原帖 → 结构化四元组（保留原帖语言，严禁翻译）。

蒸馏方式（按优先级）：
1. 会话内蒸馏（默认、首选）：由当前对话模型直接处理，不调任何 API（见 STANDARDS.md 5.3）
2. llm 模式（后备，实在不行再用）：调用 OpenAI 兼容 API 批量蒸馏
3. builtin 模式：内置规则模板转换（质量较低，仅作占位）

用法（后备模式）：
    python3 seeds/distill.py --input so_raw.json --output distilled.json --mode llm

环境变量（仅 llm 模式）：
    DISTILL_API_BASE   OpenAI 兼容 API endpoint（默认 https://api.openai.com/v1）
    DISTILL_API_KEY    API Key
    DISTILL_MODEL      模型名（默认 gpt-4o-mini）

若未设置 DISTILL_API_BASE/KEY，llm 模式会自动回退复用 .env 的智谱配置
（EMBEDDING_ENDPOINT/EMBEDDING_API_KEY，智谱 v4 接口即 OpenAI 兼容协议）。
"""

import argparse
import json
import os
import re
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path

SYSTEM_PROMPT = """你是一名资深软件工程师，负责把技术问答蒸馏成结构化的经验四元组。

输入是 Stack Overflow 问题 + 回答 或 GitHub issue + 评论。
输出必须是严格的 JSON，包含以下字段：
- problem: 一句话描述问题/症状，不要说"如何"开头，要说现象，比如 "ConnectInfo in Rust Axum always returns 127.0.0.1"
- conditions: JSON 对象，描述技术环境，只能用这些 key：technologies(数组), versions(数组), os(字符串), env(字符串), scale(字符串), language(字符串), team(字符串)
- action: 解决动作，具体可操作，不要泛泛而谈
- outcome: 结果描述，说明最终效果
- outcome_kind: 字符串，只能是 success / partial / failure / unknown
- tags: 字符串数组，3-5 个技术标签，小写

要求：
1. **保留原帖语言（最高优先级）**：英文帖输出英文四元组，中文帖输出中文四元组。只做结构化转换，**严禁翻译**——翻译会丢失报错原文、命令参数、术语细节等关键信息
2. problem 要说症状不说疑问，不要"How to solve..."，要描述现象
3. conditions 里的技术名统一小写；versions 只放带版本号的项（如 "axum 0.8"、"npm 8"、"node 18"），版本号必须原样保留；没有版本号的纯技术名放 technologies
4. conditions 不确定的字段直接省略，严禁填"unknown"等占位值——宁可整个 key 不出现
5. action 必须浓缩原帖答案里明说的具体解法（关键命令、配置项、代码要点、参数名），不得自己泛化补全"check the config"这类空话
6. outcome 只描述原帖可确认的结果（采纳答案的效果或提问者确认），不要虚构
7. 只蒸馏有明确解决结论的内容，纯讨论不要
8. 输出必须是合法 JSON，不要任何解释文字
"""


def strip_html(text: str) -> str:
    """简单去除 HTML 标签。"""
    text = re.sub(r"<pre><code>.*?</code></pre>", " [代码] ", text, flags=re.DOTALL)
    text = re.sub(r"<code>(.*?)</code>", r"\1", text, flags=re.DOTALL)
    text = re.sub(r"<[^>]+>", " ", text)
    text = re.sub(r"\s+", " ", text)
    return text.strip()


# name:version / name/version 记法（node:18、postgres:17、python/3.11）
SEP_VERSION_RE = re.compile(r"^([\w.+-]+)\s*[:/]\s*(\d[\w.+-]*)$")


def as_version_entry(v: str, from_technologies: bool = False) -> str | None:
    """判定字符串是否版本条目，返回规范化后的条目；不是则返回 None。

    - npm 8 / node 18 / diesel 0.6 / libssl1.1 → 原样保留（含任意数字即算，版本号绝不丢）
    - node:18 / postgres:17 / python/3.11 → 规范化为 "node 18" / "postgres 17"
    - a/b:latest、ubuntu/latest 等镜像 tag（分隔符后不是数字）→ None（丢弃）
    - 不含数字的纯技术名 → None（归 technologies）
    - from_technologies=True 时，单 token 数字名（es6、utf-8）不当作版本，留在 technologies
    """
    v = v.strip()
    if not v:
        return None
    m = SEP_VERSION_RE.match(v)
    if m:
        return f"{m.group(1).lower()} {m.group(2)}"
    if "/" in v or ":" in v:
        return None
    if not re.search(r"\d", v):
        return None
    if from_technologies and not re.match(r"^[\w.+-]+\s+\d", v):
        return None
    return v.lower()


def normalize_conditions(cond: dict) -> dict:
    """代码级清洗 conditions，兜底 LLM 不守规矩的输出。

    版本条目归类是确定性的、双向的（杜绝在 technologies/versions 之间摇摆）：
    - 含数字（npm 8 / node 18 / libssl1.1 都算）→ 一律进 versions，版本号原样保留
    - technologies 里混入的 "npm 8" 式条目也会被提升进 versions
    - 不含数字的纯技术名 → 一律进 technologies
    """
    if not isinstance(cond, dict):
        return {}
    cleaned = {}

    techs: list[str] = []
    versions: list[str] = []

    raw_techs = cond.get("technologies")
    for t in raw_techs if isinstance(raw_techs, list) else []:
        if not isinstance(t, str) or not t.strip():
            continue
        v = as_version_entry(t, from_technologies=True)
        if v is not None:
            versions.append(v)
        else:
            techs.append(t.strip().lower())

    raw_versions = cond.get("versions")
    for v in raw_versions if isinstance(raw_versions, list) else []:
        if not isinstance(v, str) or not v.strip():
            continue
        nv = as_version_entry(v)
        if nv is not None:
            versions.append(nv)
        elif "/" not in v and ":" not in v:
            techs.append(v.strip().lower())
        # 含 / 或 : 且非 name:version 记法的（镜像 tag 等）直接丢弃

    techs = list(dict.fromkeys(techs))
    versions = list(dict.fromkeys(versions))

    if techs:
        cleaned["technologies"] = techs
    if versions:
        cleaned["versions"] = versions
    for k in ("os", "env", "scale", "language", "team"):
        v = cond.get(k)
        if isinstance(v, str) and v.strip() and v.strip() not in ("未知", "不详", "unknown", "Unknown", "N/A"):
            cleaned[k] = v.strip()
    return cleaned


def build_prompt_so(item: dict) -> str:
    """从 SO 原始数据构建 prompt。"""
    title = item.get("title", "")
    body = strip_html(item.get("body", ""))[:2000]
    answers_text = ""
    for i, a in enumerate(item.get("answers", [])[:3]):
        ans_body = strip_html(a.get("body", ""))[:1500]
        prefix = "【最佳答案】" if a.get("is_accepted") else f"【回答{i+1}】"
        answers_text += f"{prefix}\n{ans_body}\n\n"
    return f"""Stack Overflow 问题：
标题：{title}
标签：{', '.join(item.get('tags', []))}
得分：{item.get('score', 0)}

问题正文：
{body}

{answers_text}

请蒸馏成结构化四元组 JSON（保留原帖语言，严禁翻译）。"""


def build_prompt_github(item: dict) -> str:
    """从 GitHub issue 原始数据构建 prompt。"""
    title = item.get("title", "")
    body = strip_html(item.get("body", ""))[:2000]
    labels = ", ".join(item.get("labels", []))
    comments_text = ""
    for i, c in enumerate(item.get("comments", [])[:10]):
        c_body = strip_html(c.get("body", ""))[:800]
        comments_text += f"【评论 {i+1} by {c['user']}】\n{c_body}\n\n"
    return f"""GitHub Issue：
标题：{title}
仓库：{item.get('repo', '')}
标签：{labels}
状态：{item.get('state', '')}

Issue 正文：
{body}

{comments_text}

请蒸馏成结构化四元组 JSON（保留原帖语言，严禁翻译）。只保留有明确解决结论的内容。"""


def call_llm(prompt: str, api_base: str, api_key: str, model: str) -> dict | None:
    """调用 OpenAI 兼容 API。"""
    url = f"{api_base.rstrip('/')}/chat/completions"
    payload = json.dumps({
        "model": model,
        "messages": [
            {"role": "system", "content": SYSTEM_PROMPT},
            {"role": "user", "content": prompt},
        ],
        "temperature": 0.3,
        "response_format": {"type": "json_object"},
    }).encode()
    headers = {
        "Content-Type": "application/json",
        "Authorization": f"Bearer {api_key}",
    }
    req = urllib.request.Request(url, data=payload, headers=headers, method="POST")
    try:
        with urllib.request.urlopen(req, timeout=60) as resp:
            data = json.loads(resp.read())
            content = data["choices"][0]["message"]["content"]
            # 尝试解析 JSON
            result = json.loads(content)
            return result
    except (urllib.error.HTTPError, json.JSONDecodeError, KeyError) as e:
        print(f"    LLM 调用失败: {e}", file=sys.stderr)
        return None


def builtin_distill(item: dict) -> dict | None:
    """内置简易蒸馏（仅作占位，质量有限，不推荐生产使用）。"""
    # 非常基础的规则转换，实际使用请用 llm 模式
    title = strip_html(item.get("title", ""))
    tags = item.get("tags", item.get("labels", []))
    if not title:
        return None
    return {
        "problem": title[:100],
        "conditions": {"technologies": [t.lower() for t in tags[:3]]},
        "action": "请查阅原始链接获取详细解决步骤",
        "outcome": "问题已解决",
        "outcome_kind": "success",
        "language": "zh-CN",
        "tags": [t.lower() for t in tags[:5]],
    }


def distill_item(item: dict, mode: str, api_base: str, api_key: str, model: str) -> dict | None:
    """蒸馏单条数据。"""
    source = item.get("source", "")
    if source == "stackoverflow":
        prompt = build_prompt_so(item)
    elif source == "github":
        prompt = build_prompt_github(item)
    else:
        return None

    if mode == "llm":
        result = call_llm(prompt, api_base, api_key, model)
    else:
        result = builtin_distill(item)

    if result is None:
        return None

    # 补充标准字段
    # language 按内容判定：保留原帖语言（英文帖→en，中文帖→zh-CN）
    problem_text = result.get("problem", "") or ""
    ascii_ratio = sum(1 for ch in problem_text if ord(ch) < 128) / max(len(problem_text), 1)
    result["language"] = "en" if ascii_ratio > 0.7 else "zh-CN"
    result.setdefault("outcome_kind", "success")
    if "tags" not in result or not result["tags"]:
        result["tags"] = item.get("tags", item.get("labels", []))[:5]

    # 确保 conditions 是 dict
    if not isinstance(result.get("conditions"), dict):
        result["conditions"] = {}
    # 代码级清洗（兜底 LLM 乱填：versions 无版本号、未知占位值）
    result["conditions"] = normalize_conditions(result["conditions"])

    # 添加溯源 evidence（含原作者署名，CC BY-SA 义务）
    author = item.get("owner_display_name") or item.get("user") or "匿名"
    result["evidence"] = [
        {
            "kind": "link",
            "label": f"原帖 by {author} · {source}",
            "value": item.get("link", ""),
        }
    ]

    return result


def main() -> None:
    parser = argparse.ArgumentParser(description="结构化蒸馏（API 后备模式）：原帖 → 结构化四元组，保留原帖语言；默认蒸馏方式是当前对话模型会话内处理")
    parser.add_argument("--input", required=True, help="原始数据 JSON（fetch_so/fetch_github 的输出）")
    parser.add_argument("--output", default="distilled.json", help="输出文件路径")
    parser.add_argument("--mode", choices=["llm", "builtin"], default="llm", help="蒸馏模式")
    parser.add_argument("--limit", type=int, default=0, help="最多处理条数（0=全部）")
    args = parser.parse_args()

    api_base = os.environ.get("DISTILL_API_BASE", "")
    api_key = os.environ.get("DISTILL_API_KEY", "")
    model = os.environ.get("DISTILL_MODEL", "")

    # 回退：复用 .env 的智谱配置（智谱 v4 接口即 OpenAI 兼容协议）
    if not api_base or not api_key:
        dotenv_path = Path(__file__).parent.parent / ".env"
        dotenv = {}
        if dotenv_path.exists():
            for line in dotenv_path.read_text(encoding="utf-8").splitlines():
                line = line.strip()
                if line and not line.startswith("#") and "=" in line:
                    k, _, v = line.partition("=")
                    dotenv[k.strip()] = v.strip().strip('"').strip("'")
        endpoint = dotenv.get("EMBEDDING_ENDPOINT", "")
        key = dotenv.get("EMBEDDING_API_KEY", "")
        if endpoint and key:
            if not api_base:
                # 智谱 chat 接口：把 .../v4/embeddings 规范化为 .../v4（chat/completions 由 call_llm 拼接）
                base = endpoint.split("/chat/completions")[0]
                if base.endswith("/embeddings"):
                    base = base[: -len("/embeddings")]
                api_base = base
            if not api_key:
                api_key = key
        if not model:
            model = "glm-4-flash"  # 聊天蒸馏模型，注意不能复用 EMBEDDING_MODEL（那是 embedding 模型）

    if args.mode == "llm" and not api_key:
        print("错误：llm 模式需要 DISTILL_API_KEY 环境变量", file=sys.stderr)
        sys.exit(1)

    in_path = Path(args.input)
    raw_items = json.loads(in_path.read_text(encoding="utf-8"))
    if args.limit > 0:
        raw_items = raw_items[:args.limit]

    print(f"待蒸馏: {len(raw_items)} 条")
    print(f"模式: {args.mode}")

    results: list[dict] = []
    failures = 0

    for i, item in enumerate(raw_items, 1):
        source_id = item.get("question_id") or item.get("issue_number", "?")
        print(f"  [{i}/{len(raw_items)}] {item.get('source', '?')}#{source_id}...", end=" ", flush=True)
        result = distill_item(item, args.mode, api_base, api_key, model)
        if result:
            results.append(result)
            print("✓")
        else:
            failures += 1
            print("✗")
        if args.mode == "llm" and i % 10 == 0:
            time.sleep(1)  # 每 10 条歇一下

    out_path = Path(args.output)
    out_path.write_text(json.dumps(results, ensure_ascii=False, indent=2), encoding="utf-8")
    print(f"\n完成：成功 {len(results)} 条，失败 {failures} 条，已保存到 {out_path}")


if __name__ == "__main__":
    main()
