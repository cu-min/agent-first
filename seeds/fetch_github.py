#!/usr/bin/env python3
"""GitHub closed issues 抓取器（知名仓库）。

用法：
    python3 seeds/fetch_github.py --repos tokio-rs/tokio,facebook/react --pages 3 --output gh_raw.json

输出 JSON 结构：
[
  {
    "source": "github",
    "repo": "tokio-rs/tokio",
    "issue_number": 1234,
    "title": "...",
    "body": "...",
    "labels": ["bug", "A-tokio"],
    "state": "closed",
    "comments": [
      {"user": "maintainer", "body": "...", "created_at": "..."},
      ...
    ],
    "link": "https://github.com/tokio-rs/tokio/issues/1234",
    "user": "reporter",
    "closed_at": "...",
    "created_at": "..."
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

GITHUB_API = "https://api.github.com"


def gh_request(path: str, token: str | None = None) -> dict | list:
    url = f"{GITHUB_API}{path}"
    headers = {
        "User-Agent": "agent-first-seed-importer/1.0",
        "Accept": "application/vnd.github.v3+json",
    }
    if token:
        headers["Authorization"] = f"Bearer {token}"
    req = urllib.request.Request(url, headers=headers)
    try:
        with urllib.request.urlopen(req, timeout=30) as resp:
            return json.loads(resp.read())
    except urllib.error.HTTPError as e:
        if e.code == 403 and "rate limit" in e.read().decode().lower():
            print(f"  GitHub 限流，等待 60s...", file=sys.stderr)
            time.sleep(60)
            return gh_request(path, token)
        print(f"  GitHub API 错误 {e.code}: {e.read().decode()}", file=sys.stderr)
        return {}


def fetch_issues(repo: str, page: int, token: str | None = None, labels: str = "", min_comments: int = 3) -> list[dict]:
    """用 search API 只搜真实 issue（自动排除 PR），已关闭且有足够讨论。"""
    query = f"repo:{repo} is:issue is:closed comments:>={min_comments}"
    if labels:
        query += f" label:{labels}"
    params = {
        "q": query,
        "sort": "comments",
        "order": "desc",
        "per_page": 20,
        "page": str(page),
    }
    qs = urllib.parse.urlencode(params)
    data = gh_request(f"/search/issues?{qs}", token)
    if isinstance(data, dict):
        return data.get("items", []) or []
    return []


def fetch_comments(repo: str, issue_number: int, token: str | None = None) -> list[dict]:
    """抓取 issue 评论（前 30 条）。"""
    params = {"per_page": 30, "page": 1, "sort": "created", "direction": "asc"}
    qs = urllib.parse.urlencode(params)
    return gh_request(f"/repos/{repo}/issues/{issue_number}/comments?{qs}", token) or []


def main() -> None:
    parser = argparse.ArgumentParser(description="抓取 GitHub closed issues")
    parser.add_argument("--repos", default="tokio-rs/tokio,facebook/react,spring-projects/spring-boot,vercel/next.js,docker/compose,psf/requests",
                        help="仓库列表，逗号分隔（owner/repo 格式）")
    parser.add_argument("--pages", type=int, default=3, help="每个仓库抓取几页")
    parser.add_argument("--token", default=None, help="GitHub token（可选，提升限流）")
    parser.add_argument("--min-comments", type=int, default=3, help="最少评论数，低于此值跳过")
    parser.add_argument("--output", default="gh_raw.json", help="输出文件路径")
    args = parser.parse_args()

    repos = [r.strip() for r in args.repos.split(",") if r.strip()]
    all_items: list[dict] = []
    seen_keys: set[str] = set()

    print(f"抓取仓库: {repos}")
    print(f"每个仓库 {args.pages} 页，每页约 20 条")

    for repo in repos:
        print(f"\n处理仓库: {repo}")
        for page in range(1, args.pages + 1):
            print(f"  第 {page}/{args.pages} 页...", end=" ", flush=True)
            issues = fetch_issues(repo, page, args.token, min_comments=args.min_comments)
            if not issues:
                print("无数据")
                break

            new_count = 0
            for issue in issues:
                # 跳过 PR（GitHub API 把 PR 也放在 issues 里返回）
                if "pull_request" in issue:
                    continue
                if issue.get("comments", 0) < args.min_comments:
                    continue
                key = f"{repo}#{issue['number']}"
                if key in seen_keys:
                    continue
                seen_keys.add(key)

                # 抓取评论
                comments_raw = fetch_comments(repo, issue["number"], args.token)
                comments = [
                    {
                        "user": c.get("user", {}).get("login", ""),
                        "body": c.get("body", ""),
                        "created_at": c.get("created_at", ""),
                    }
                    for c in comments_raw
                ]

                all_items.append({
                    "source": "github",
                    "repo": repo,
                    "issue_number": issue["number"],
                    "title": issue.get("title", ""),
                    "body": issue.get("body", "") or "",
                    "labels": [l["name"] for l in issue.get("labels", [])],
                    "state": issue.get("state", ""),
                    "comments": comments,
                    "link": issue.get("html_url", ""),
                    "user": issue.get("user", {}).get("login", ""),
                    "closed_at": issue.get("closed_at", ""),
                    "created_at": issue.get("created_at", ""),
                })
                new_count += 1
                time.sleep(0.3)

            print(f"新增 {new_count} 条（累计 {len(all_items)}）")
            time.sleep(1)

    out_path = Path(args.output)
    out_path.write_text(json.dumps(all_items, ensure_ascii=False, indent=2), encoding="utf-8")
    print(f"\n完成，共 {len(all_items)} 条，已保存到 {out_path}")


if __name__ == "__main__":
    main()
