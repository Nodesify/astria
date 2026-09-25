# Security Policy

astria is built to run against **untrusted input**: repositories you don't own, URLs you didn't choose, and exports you open in a browser. If you find a way to break that boundary, we want to hear about it.

## Supported versions

| Version | Supported |
|---|---|
| 1.0.x | ✅ |
| < 1.0 (`@nodesify/graphify`) | ❌ — please upgrade and run `astria migrate` |

## Reporting a vulnerability

**Please do not open a public issue for security problems.**

- Use GitHub's **Private vulnerability reporting** for this repository (*Security → Report a vulnerability*), or
- Contact the maintainers via [nodesify.com](https://nodesify.com).

We aim to acknowledge reports within **72 hours** and will work with you on a coordinated disclosure timeline. Credit is given unless you prefer to remain anonymous.

## Threat model highlights

What astria is designed to withstand (and what we most want tested):

- **Untrusted repositories** — sensitive-path denylisting (`.env`, keys, credentials), minified/vendored asset skipping, no shell-string execution, literal-allowlist native module loading, install-path containment.
- **Hostile URLs** (`astria add <url>`) — `http`/`https` only; every redirect hop re-validated; DNS-resolved hosts checked against loopback/private/CGNAT/link-local ranges (IPv4 + IPv6, including mapped forms); download size and timeout caps; slugified filenames so a URL segment cannot write outside `.astria/raw/`.
- **Untrusted content in exports** — node/community labels are rendered strictly as text in every HTML export; no HTML interpolation.
- **Secret hygiene** — MCP config ingestion records env var **names**, never values; LLM API keys are sent in request headers (never in URLs); a plain-`http` non-local LLM base URL with a key configured prints a warning.

Out of scope: the security of third-party LLM backends you configure, and the `psql` binary used for `add --postgres`.
