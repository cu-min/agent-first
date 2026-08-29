#!/usr/bin/env python3
"""导入盲测幸存记忆（150 条蒸馏数据中盲测判不等价的 9 条）。

复用 import_seeds.py 的流程：注册 Agent → 认领工作区 → 公开策略 auto →
单批导入（9 条 < 100 条/次上限，无需分批）。

用法：python seeds/import_survivors.py [BASE_URL]
"""

import json
import secrets
import sys
import urllib.error
import urllib.request
from pathlib import Path

BASE = sys.argv[1].rstrip("/") if len(sys.argv) > 1 else "http://127.0.0.1:8080"
SEED_FILE = Path(__file__).parent / "_survivors_final.json"


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
    print(f"加载 {len(memories)} 条盲测幸存记忆")

    agent = request("POST", "/v1/agents/register", {"name": "seed-importer"})
    if not agent.get("claim_token"):
        raise SystemExit("注册返回缺少 claim_token")
    print(f"agent_id={agent['agent_id']}")

    login_name = f"seed-{secrets.token_hex(4)}"
    password = secrets.token_urlsafe(16)
    session = request(
        "POST",
        "/v1/developers/claim",
        {"claim_token": agent["claim_token"], "login_name": login_name, "password": password},
    )
    developer_token = session["developer_token"]
    workspace_id = request("GET", "/v1/developer/overview", token=developer_token)["workspaces"][0]["id"]
    print(f"workspace_id={workspace_id}（登录名 {login_name}）")

    request(
        "POST",
        f"/v1/workspaces/{workspace_id}/publication-policy",
        {"publication_policy": "auto"},
        token=developer_token,
    )

    for memory in memories:
        memory["request_public"] = True
    imported = request(
        "POST",
        "/v1/memories/import",
        {"memories": memories},
        token=agent["api_key"],
    )
    print(f"导入成功：{imported['imported']} 条")

    result = request("POST", "/v1/search", {"query": "VS Code old TypeScript version JSX error", "limit": 3})
    print(f"验证检索：命中 {len(result['items'])} 条，模式 {result['retrieval']}")
    for item in result["items"]:
        print(f"  [{item['outcome_kind']}] {item['problem'][:70]}")


if __name__ == "__main__":
    main()
