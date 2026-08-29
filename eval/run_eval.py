"""agent-first 检索质量评测 harness。

用法：
    python eval/run_eval.py --label "baseline"
    python eval/run_eval.py --label "private" --key-file research/_agent_key.json

指标：
    正例（应命中）：hit@1 / hit@5 / MRR，按查询风格（error/paraphrase/keyword/cross）分组
    负例（应无精确命中）：空返回率、返回但全 related（分级拦截）、exact 泄漏（真泄漏），按 non_tech / tech_absent 分组

评测查询只是工具，绝不写入记忆网络。改检索逻辑（阈值/分词/排序）前后各跑一次对比。
"""
import argparse
import json
import time
import urllib.error
import urllib.request
from collections import defaultdict

DEFAULT_BASE = 'http://127.0.0.1:8080'


def search(base, query, key=None, limit=5, throttle=1.1):
    headers = {'Content-Type': 'application/json'}
    if key:
        headers['Authorization'] = 'Bearer ' + key
    time.sleep(throttle)
    req = urllib.request.Request(
        base + '/v1/search',
        data=json.dumps({'query': query, 'limit': limit}).encode('utf-8'),
        headers=headers,
        method='POST',
    )
    for attempt in range(3):
        try:
            with urllib.request.urlopen(req, timeout=30) as r:
                return json.load(r).get('items', [])
        except urllib.error.HTTPError as e:
            if e.code == 429 and attempt < 2:
                time.sleep(65)
                continue
            raise RuntimeError(f'查询失败 [{e.code}]: {query!r} -> {e.read().decode("utf-8", "replace")[:200]}')


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--base-url', default=DEFAULT_BASE)
    parser.add_argument('--queries', default='eval/queries.json')
    parser.add_argument('--key-file', default=None, help='带 Agent Key 的 JSON（含 api_key），评测私有语料时使用')
    parser.add_argument('--label', default='run', help='本次运行标签，用于结果文件命名')
    parser.add_argument('--out-dir', default='research', help='结果 JSON 输出目录')
    parser.add_argument('--limit', type=int, default=5)
    args = parser.parse_args()

    key = None
    if args.key_file:
        with open(args.key_file, encoding='utf-8') as f:
            key = json.load(f)['api_key']

    with open(args.queries, encoding='utf-8') as f:
        data = json.load(f)
    positives = data['positives']
    negatives = data['negatives']

    print(f'评测集: {len(positives)} 正例 / {len(negatives)} 负例 | 身份: {"Agent Key" if key else "匿名(公开语料)"}')

    positive_rows = []
    for i, case in enumerate(positives):
        hits = search(args.base_url, case['query'], key, args.limit)
        rank = next((idx + 1 for idx, h in enumerate(hits) if case['expect'] in h['problem']), None)
        positive_rows.append({'query': case['query'], 'style': case['style'], 'expect': case['expect'],
                              'rank': rank, 'returned': len(hits),
                              'top1': hits[0]['problem'][:70] if hits else ''})
        if (i + 1) % 20 == 0:
            print(f'  正例进度 {i + 1}/{len(positives)}')

    negative_rows = []
    for case in negatives:
        hits = search(args.base_url, case['query'], key, args.limit)
        negative_rows.append({'query': case['query'], 'category': case['category'],
                              'returned': len(hits),
                              'exact_leak': any(h.get('relevance') == 'exact' for h in hits),
                              'top1': hits[0]['problem'][:70] if hits else ''})

    total = len(positive_rows)
    hit1 = sum(1 for r in positive_rows if r['rank'] == 1)
    hit5 = sum(1 for r in positive_rows if r['rank'] is not None and r['rank'] <= 5)
    mrr = sum(1 / r['rank'] for r in positive_rows if r['rank'] is not None) / total

    print()
    print(f'=== 正例召回 (label={args.label}) ===')
    print(f'  总体: hit@1 {hit1}/{total} ({hit1 / total:.1%}) | hit@5 {hit5}/{total} ({hit5 / total:.1%}) | MRR {mrr:.3f}')
    by_style = defaultdict(list)
    for r in positive_rows:
        by_style[r['style']].append(r)
    for style, rows in sorted(by_style.items()):
        h1 = sum(1 for r in rows if r['rank'] == 1)
        h5 = sum(1 for r in rows if r['rank'] is not None and r['rank'] <= 5)
        m = sum(1 / r['rank'] for r in rows if r['rank'] is not None) / len(rows)
        print(f'  {style:10}: hit@1 {h1}/{len(rows)} ({h1 / len(rows):.1%}) | hit@5 {h5}/{len(rows)} ({h5 / len(rows):.1%}) | MRR {m:.3f}')

    misses = [r for r in positive_rows if r['rank'] is None]
    if misses:
        print(f'  未命中 {len(misses)} 条:')
        for r in misses:
            print(f'    [{r["style"]}] {r["query"][:46]:48} top1: {r["top1"][:44] or "(空)"}')

    print()
    print(f'=== 负例泄漏 (label={args.label}) ===')
    by_cat = defaultdict(list)
    for r in negative_rows:
        by_cat[r['category']].append(r)
    for cat, rows in sorted(by_cat.items()):
        empty = sum(1 for r in rows if r['returned'] == 0)
        graded = sum(1 for r in rows if r['returned'] > 0 and not r.get('exact_leak'))
        exact_leak = sum(1 for r in rows if r.get('exact_leak'))
        print(f'  {cat:12}: 空返回 {empty}/{len(rows)} | 全 related（分级拦截） {graded}/{len(rows)} | exact 泄漏 {exact_leak}/{len(rows)}')
        for r in rows:
            if r['returned'] > 0:
                mark = 'EXACT!' if r.get('exact_leak') else 'related'
                print(f'    [{mark}] {r["query"][:40]:42} -> {r["returned"]} 条, top1: {r["top1"][:40]}')

    result = {
        'label': args.label,
        'timestamp': time.strftime('%Y-%m-%d %H:%M:%S'),
        'identity': 'agent_key' if key else 'anonymous',
        'positives': {'total': total, 'hit1': hit1, 'hit5': hit5, 'mrr': round(mrr, 4)},
        'positive_rows': positive_rows,
        'negative_rows': negative_rows,
    }
    out_path = f'{args.out_dir}/_eval_{args.label}.json'
    with open(out_path, 'w', encoding='utf-8') as f:
        json.dump(result, f, ensure_ascii=False, indent=1)
    print(f'\n结果已存 {out_path}')


if __name__ == '__main__':
    main()
