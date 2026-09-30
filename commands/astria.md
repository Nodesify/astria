---
description: Query the astria codebase knowledge graph (builds or refreshes it if missing)
---

Query the astria knowledge graph for this repository: $ARGUMENTS

1. If `.astria/` does not exist yet, run `astria run .` to build the graph (ask first if the repo is large); if it exists but may be stale, run `astria update .` first. Say which you did.
2. If the `astria` CLI is not installed and `.astria/` does not exist, tell the user it isn't installed and offer `npm install -g @nodesify/astria` — ask before installing, and do not fake graph answers from raw file reads.
3. Prefer the graph over grep/glob for this question: `astria query "<question>"` for where/how questions, `astria map` for orientation, `astria explain <node>` for a specific symbol, `astria path <A> <B>` for connections, `astria affected <node>` for blast radius, `--detail high` when only declared facts should ground the answer.
4. Read the `file:line` anchors the graph returns before drawing conclusions, and cite them in the answer.
5. Close with the shortest ordered list of files a human should open, each with a one-line reason.
