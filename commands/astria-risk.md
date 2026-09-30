---
description: Map the current git diff onto the astria graph and produce a PR-ready risk report
---

Produce a blast-radius risk report for the current diff using the astria knowledge graph.

1. If `.astria/` does not exist, run `astria run .` first (ask first if the repo is large); if it may be stale, run `astria update .`.
2. Run `astria risk` and read its output.
3. Summarize for review: which changed symbols carry the widest reverse reachability, which hub files or communities the diff touches, and the top three review priorities with `file:line` anchors.
4. State clearly what the graph could not see (uncommitted-new files, generated code, excluded paths).
