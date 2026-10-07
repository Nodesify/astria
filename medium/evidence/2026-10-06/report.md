# Paired retrieval measurements

2026-10-06T09:04:34.556Z; schema 3. Each corpus/split is reported separately. Single measurements include process startup; no statistical superiority claim.

## astria-self (frozen-v1)

Corpus 5e69c63cee176a7addc3b5c4414280bba3d2d815; golden SHA-256 a5f0769fefba93256cb973129c2b3dca172478ea255fb9bf2c5cecde6e11ed8f.

| Tool | Budget | Questions | File recall@5 | File MRR | Def recall@5 | Def MRR | Avg tokens | Raw over budget | Errors |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| astria | 1000 | 35 | 75.7% | 0.570 | n/a | n/a | 965 | 0 | 0 |
| graphify | 1000 | 35 | 55.7% | 0.464 | n/a | n/a | 930 | 24 | 0 |
| astria | 4000 | 35 | 78.6% | 0.586 | n/a | n/a | 3971 | 0 | 0 |
| graphify | 4000 | 35 | 55.7% | 0.464 | n/a | n/a | 3491 | 22 | 0 |

graphify: build 13.598 s; grounded definitions present 0/0.

astria: build 15.805 s; grounded definitions present 0/0. Exact cached code IDs preserved: 2642/2642.

## astria-self-v2 (corrected-v2)

Corpus 5e69c63cee176a7addc3b5c4414280bba3d2d815; golden SHA-256 683a2b44f15e697dda7f75665381bcd94c509da118e9ca622872f5a1d237cab2.

| Tool | Budget | Questions | File recall@5 | File MRR | Def recall@5 | Def MRR | Avg tokens | Raw over budget | Errors |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| astria | 1000 | 35 | 75.7% | 0.570 | n/a | n/a | 965 | 0 | 0 |
| graphify | 1000 | 35 | 56.7% | 0.467 | n/a | n/a | 931 | 24 | 0 |
| astria | 4000 | 35 | 78.6% | 0.586 | n/a | n/a | 3972 | 0 | 0 |
| graphify | 4000 | 35 | 56.7% | 0.467 | n/a | n/a | 3490 | 22 | 0 |

graphify: build 13.194 s; grounded definitions present 0/0.

astria: build 19.603 s; grounded definitions present 0/0. Exact cached code IDs preserved: 2642/2642.

## click (frozen-v1)

Corpus 934813e4d421071a1b3db3973c02fe2721359a6e; golden SHA-256 7023c4f03b3c7537e613d1d9fcfab6abfcea627910b940910057139ebee0fe13.

| Tool | Budget | Questions | File recall@5 | File MRR | Def recall@5 | Def MRR | Avg tokens | Raw over budget | Errors |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| astria | 1000 | 3 | 100.0% | 1.000 | 60.0% | 0.354 | 976 | 0 | 0 |
| graphify | 1000 | 3 | 100.0% | 0.667 | 0.0% | 0.008 | 711 | 2 | 0 |
| astria | 4000 | 3 | 100.0% | 1.000 | 60.0% | 0.354 | 3981 | 0 | 0 |
| graphify | 4000 | 3 | 100.0% | 0.667 | 0.0% | 0.008 | 2456 | 0 | 0 |

graphify: build 6.472 s; grounded definitions present 4/5.

astria: build 5.606 s; grounded definitions present 5/5. Exact cached code IDs preserved: 1148/1148.

Definition misses (outside top five), astria @ 1000:
- click1 BaseCommand.invoke: rank 8
- click1 MultiCommand.invoke: rank 9

Definition misses (outside top five), graphify @ 1000:
- click1 Context.invoke: not returned
- click1 BaseCommand.invoke: not returned
- click1 Command.invoke: not returned
- click1 MultiCommand.invoke: not returned
- click2 MultiCommand.resolve_command: rank 24

Definition misses (outside top five), astria @ 4000:
- click1 BaseCommand.invoke: rank 8
- click1 MultiCommand.invoke: rank 9

Definition misses (outside top five), graphify @ 4000:
- click1 Context.invoke: not returned
- click1 BaseCommand.invoke: not returned
- click1 Command.invoke: not returned
- click1 MultiCommand.invoke: not returned
- click2 MultiCommand.resolve_command: rank 24

## click-heldout (additional-validation-v1)

Corpus 934813e4d421071a1b3db3973c02fe2721359a6e; golden SHA-256 2d1b5f7a0a43c13d5796b370766662ddf347b3d9ffead93c3cf7a8db5e338dba.

| Tool | Budget | Questions | File recall@5 | File MRR | Def recall@5 | Def MRR | Avg tokens | Raw over budget | Errors |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| astria | 1000 | 2 | 100.0% | 0.750 | 100.0% | 0.625 | 971 | 0 | 0 |
| graphify | 1000 | 2 | 50.0% | 0.500 | 0.0% | 0.000 | 989 | 2 | 0 |
| astria | 4000 | 2 | 100.0% | 0.750 | 100.0% | 0.625 | 3974 | 0 | 0 |
| graphify | 4000 | 2 | 50.0% | 0.500 | 0.0% | 0.010 | 3665 | 0 | 0 |

graphify: build 5.203 s; grounded definitions present 2/2.

astria: build 5.994 s; grounded definitions present 2/2. Exact cached code IDs preserved: 1148/1148.

Definition misses (outside top five), graphify @ 1000:
- heldout-overrides BaseCommand.get_usage: not returned
- heldout-overrides Command.get_usage: not returned

Definition misses (outside top five), graphify @ 4000:
- heldout-overrides BaseCommand.get_usage: rank 115
- heldout-overrides Command.get_usage: rank 95

## express (frozen-v1)

Corpus 1faf228935aa0a13111f92c28ee795be64ce3f0f; golden SHA-256 704eff15ea9ab24473d562657525df1f033199cbb280967d5717a483d0824b9d.

| Tool | Budget | Questions | File recall@5 | File MRR | Def recall@5 | Def MRR | Avg tokens | Raw over budget | Errors |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| astria | 1000 | 2 | 100.0% | 1.000 | 100.0% | 1.000 | 988 | 0 | 0 |
| graphify | 1000 | 2 | 100.0% | 0.625 | 50.0% | 0.100 | 973 | 1 | 0 |
| astria | 4000 | 2 | 100.0% | 1.000 | 100.0% | 1.000 | 3939 | 0 | 0 |
| graphify | 4000 | 2 | 100.0% | 0.625 | 50.0% | 0.107 | 3079 | 1 | 0 |

graphify: build 8.734 s; grounded definitions present 2/2.

astria: build 12.132 s; grounded definitions present 2/2. Exact cached code IDs preserved: 3263/3263.

Definition misses (outside top five), graphify @ 1000:
- express1 res.json: not returned

Definition misses (outside top five), graphify @ 4000:
- express1 res.json: rank 73

## express-heldout (additional-validation-v1)

Corpus 1faf228935aa0a13111f92c28ee795be64ce3f0f; golden SHA-256 74e6f8c84932f6f7711027121b5c9517207323dba955404b870df39492dff8c1.

| Tool | Budget | Questions | File recall@5 | File MRR | Def recall@5 | Def MRR | Avg tokens | Raw over budget | Errors |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| astria | 1000 | 2 | 100.0% | 0.667 | 100.0% | 1.000 | 977 | 0 | 0 |
| graphify | 1000 | 2 | 50.0% | 0.500 | 100.0% | 0.333 | 963 | 0 | 0 |
| astria | 4000 | 2 | 100.0% | 0.667 | 100.0% | 1.000 | 3986 | 0 | 0 |
| graphify | 4000 | 2 | 50.0% | 0.500 | 100.0% | 0.333 | 3994 | 2 | 0 |

graphify: build 4.390 s; grounded definitions present 1/1.

astria: build 10.792 s; grounded definitions present 1/1. Exact cached code IDs preserved: 3263/3263.

## ripgrep (frozen-v1)

Corpus 4649aa9700619f94cf9c66876e9549d83420e16c; golden SHA-256 00966efb5657172de9761fb78e1f760ef6a64899754ff428989f4afe4c4d5da4.

| Tool | Budget | Questions | File recall@5 | File MRR | Def recall@5 | Def MRR | Avg tokens | Raw over budget | Errors |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| astria | 1000 | 3 | 100.0% | 0.778 | n/a | n/a | 986 | 0 | 0 |
| graphify | 1000 | 3 | 100.0% | 0.833 | n/a | n/a | 994 | 3 | 0 |
| astria | 4000 | 3 | 100.0% | 0.778 | n/a | n/a | 3972 | 0 | 0 |
| graphify | 4000 | 3 | 100.0% | 0.833 | n/a | n/a | 3830 | 0 | 0 |

graphify: build 8.259 s; grounded definitions present 0/0.

astria: build 11.908 s; grounded definitions present 0/0. Exact cached code IDs preserved: 3091/3091.

## ripgrep-heldout (additional-validation-v1)

Corpus 4649aa9700619f94cf9c66876e9549d83420e16c; golden SHA-256 fd2ecce3a03a951cbc6323fb0b2654dc750843a9100731ce08ee9ed6eb162535.

| Tool | Budget | Questions | File recall@5 | File MRR | Def recall@5 | Def MRR | Avg tokens | Raw over budget | Errors |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| astria | 1000 | 3 | 100.0% | 0.667 | 50.0% | 0.411 | 965 | 0 | 0 |
| graphify | 1000 | 3 | 33.3% | 0.333 | 0.0% | 0.028 | 988 | 3 | 0 |
| astria | 4000 | 3 | 100.0% | 0.667 | 50.0% | 0.411 | 3982 | 0 | 0 |
| graphify | 4000 | 3 | 33.3% | 0.333 | 0.0% | 0.028 | 3990 | 3 | 0 |

graphify: build 7.079 s; grounded definitions present 4/4.

astria: build 12.707 s; grounded definitions present 4/4. Exact cached code IDs preserved: 3091/3091.

Definition misses (outside top five), astria @ 1000:
- heldout-compound RegexMatcherBuilder.case_insensitive: not returned
- heldout-flow RegexMatcherBuilder.build_many: rank 7

Definition misses (outside top five), graphify @ 1000:
- heldout-inline-test tests::case_smart: not returned
- heldout-compound RegexMatcherBuilder.case_insensitive: not returned
- heldout-flow RegexMatcherBuilder.build_many: rank 16
- heldout-flow Config.build_many: rank 20

Definition misses (outside top five), astria @ 4000:
- heldout-compound RegexMatcherBuilder.case_insensitive: not returned
- heldout-flow RegexMatcherBuilder.build_many: rank 7

Definition misses (outside top five), graphify @ 4000:
- heldout-inline-test tests::case_smart: not returned
- heldout-compound RegexMatcherBuilder.case_insensitive: not returned
- heldout-flow RegexMatcherBuilder.build_many: rank 16
- heldout-flow Config.build_many: rank 20

Definition ranks count returned nodes, file ranks count distinct paths. Presence matches declaration file and line; retention matches exact scoped extraction IDs. File hits do not imply that the right symbol survived. No answer correctness was measured.

