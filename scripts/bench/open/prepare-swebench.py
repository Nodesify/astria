#!/usr/bin/env python
"""Fetch SWE-bench Verified into a local JSONL via the HF datasets-server.

Usage: python prepare-swebench.py
Output: bench-work/open/swebench_verified.jsonl (500 rows).
Dataset: princeton-nlp/SWE-bench_Verified (MIT). Not redistributed.
"""
import json
import urllib.request
from pathlib import Path

out = Path(__file__).resolve().parents[3] / 'bench-work' / 'open' / 'swebench_verified.jsonl'
rows, offset = [], 0
while True:
    url = (
        'https://datasets-server.huggingface.co/rows'
        '?dataset=princeton-nlp%2FSWE-bench_Verified&config=default&split=test'
        f'&offset={offset}&length=100'
    )
    with urllib.request.urlopen(url, timeout=60) as r:
        d = json.load(r)
    rows.extend(x['row'] for x in d['rows'])
    offset += 100
    if offset >= d['num_rows_total']:
        break
out.write_text('\n'.join(json.dumps({
    'instance_id': r['instance_id'], 'repo': r['repo'], 'base_commit': r['base_commit'],
    'problem_statement': r['problem_statement'], 'patch': r['patch'], 'test_patch': r['test_patch'],
}, ensure_ascii=False) for r in rows), encoding='utf8')
print(f'wrote {len(rows)} rows -> {out}')
