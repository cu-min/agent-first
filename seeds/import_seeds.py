#!/usr/bin/env python3
"""向 Agent-first 灌入种子记忆（解决空库冷启动）。

流程：注册 Agent → 用 claim_token 认领工作区（创建开发者账号）→
公开策略设为 auto → 通过 /v1/memories/import 批量导入公开记忆 → 验证检索。

用法：
    python3 seeds/import_seeds.py http://localhost:8080
    python3 seeds/import_seeds.py https://your-domain.com

依赖：仅 Python 标准库。
"""

import json
import secrets
import sys
import urllib.error
import urllib.request
from pathlib import Path

BASE = sys.argv[1].rstrip("/") if len(sys.argv) > 1 else "http://localhost:8080"
SEED_FILE = Path(__file__).parent / "seed_memories.json"


def request(method: str, path: str, body: dict | None = None, token: str | None = None) -> dict:
    payload = json.dumps(body).encode() if body is not None else None
    headers = {"Content-Type": "application/json"}
    if token:
        headers["Authorization"] = f"Bearer {token}"
    req = urllib.request.Request(f"{BASE}{path}", data=payload, headers=headers, method=method)
    try:
        with urllib.request.urlopen(req, timeout=30) as response:
            raw = response.read()
            return json.loads(raw) if raw else {}
    except urllib.error.HTTPError as error:
        detail = error.read().decode(errors="replace")
        raise SystemExit(f"请求失败 {error.code} {path}: {detail}") from error


def main() -> None:
    memories = json.loads(SEED_FILE.read_text(encoding="utf-8"))
    print(f"加载 {len(memories)} 条种子记忆")

    # 1. 注册 Agent（独立工作区）
    agent = request("POST", "/v1/agents/register", {"name": "seed-importer"})
    if not agent.get("claim_token"):
        raise SystemExit("注册返回缺少 claim_token，无法认领工作区")
    print(f"agent_id={agent['agent_id']}")

    # 2. 认领工作区，成为开发者
    login_name = f"seed-{secrets.token_hex(4)}"
    password = secrets.token_urlsafe(16)
    session = request(
        "POST",
        "/v1/developers/claim",
        {
            "claim_token": agent["claim_token"],
            "login_name": login_name,
            "password": password,
        },
    )
    developer_token = session["developer_token"]
    workspace_id = request("GET", "/v1/developer/overview", token=developer_token)["workspaces"][0]["id"]
    print(f"workspace_id={workspace_id}（登录名 {login_name}，密码 {password}，仅本机展示）")

    # 3. 公开策略设为 auto，request_public 的记忆直接发布
    request(
        "POST",
        f"/v1/workspaces/{workspace_id}/publication-policy",
        {"publication_policy": "auto"},
        token=developer_token,
    )

    # 4. 标记 request_public 后批量导入
    for memory in memories:
        memory["request_public"] = True
    imported = request(
        "POST",
        "/v1/memories/import",
        {"memories": memories},
        token=agent["api_key"],
    )
    print(f"导入成功：{imported['imported']} 条")

    # 5. 验证匿名检索（public 记忆无需认证）
    result = request("POST", "/v1/search", {"query": "Docker 连接 PostgreSQL 拒绝", "limit": 3})
    print(f"验证检索（Docker/PostgreSQL）：命中 {len(result['items'])} 条，检索模式 {result['retrieval']}")
    for item in result["items"]:
        print(f"  [{item['outcome_kind']}] {item['problem']}")

    result = request("POST", "/v1/search", {"query": "JWT 401 时钟", "limit": 3})
    print(f"验证检索（JWT/时钟）：命中 {len(result['items'])} 条")


if __name__ == "__main__":
    main()
