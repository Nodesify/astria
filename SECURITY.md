# Security Policy

astria is built to run against **untrusted input**: repositories you don't own, URLs you didn't choose, and exports you open in a browser. If you find a way to break that boundary, we want to hear about it.

## Supported versions

| Version | Supported |
|---|---|
| 1.x (latest patch release) | ✅ |
| < 1.0 (`@nodesify/graphify`) | ❌ — please upgrade and run `astria migrate` |

## Reporting a vulnerability

**Please do not open a public issue for security problems.**

- Use GitHub's **Private vulnerability reporting** for this repository (*Security → Report a vulnerability*), or
- Contact the maintainers via [nodesify.com](https://nodesify.com).

We aim to acknowledge reports within **72 hours** and will work with you on a coordinated disclosure timeline. Credit is given unless you prefer to remain anonymous.

## Threat model highlights

What astria is designed to withstand (and what we most want tested):

- **Untrusted repositories** — sensitive-path denylisting (`.env` in either spelling, keys, certificates, keystores, Terraform state, fixed-name credential files such as `.netrc`, `.gitconfig` and `.pypirc`), minified/vendored asset skipping, no shell-string execution, literal-allowlist native module loading, install-path containment. Symbolic links are never followed — neither a file's target nor into a directory — so a repository cannot name a file outside the scanned tree. Paths named inside manifests (Cargo workspace members and path dependencies, JavaScript workspace globs and package entry fields) are resolved only when they stay inside the scanned tree; absolute, rooted, drive-relative and UNC spellings are refused before the operating system sees them.
- **Hostile media files** — video and audio are decoded by `ffmpeg` and `whisper-cli`, called with an argument list (no shell), each run under a wall-clock deadline so a file that stalls a decoder cannot hold the build open. What the tools write goes to a scratch directory created for the one attempt, readable by its owner only and removed afterwards.
- **Hostile URLs** (`astria add <url>`) — `http`/`https` only; every redirect hop re-validated; DNS-resolved hosts checked against loopback/private/CGNAT/link-local ranges (IPv4 + IPv6, including mapped forms); download size and timeout caps; slugified filenames so a URL segment cannot write outside `raw/`. Media URLs are the exception: every destination yt-dlp resolves is checked against the same ranges before the download starts, but the download then runs inside yt-dlp, whose own redirects that check cannot see. A host answering the two requests differently can send the downloader to an internal address; it is bounded by one file per URL, a size cap, a socket timeout and a deadline, and what comes back is treated as media to transcribe, nothing more. Ingest media only from hosts you would fetch a page from.
- **Untrusted content in exports** — node/community labels are rendered strictly as text in every HTML export; no HTML interpolation.
- **Secret hygiene** — MCP config ingestion records env var **names**, never values; LLM API keys are sent in request headers (never in URLs); a plain-`http` non-local LLM base URL with a key configured prints a warning.
- **Dependencies** — `cargo audit` and `npm audit` (CLI and docs site) run on every change, as does `cargo deny` for the license and source policy. The exception list (`.cargo/audit.toml`) carries only advisories on code astria never links, each with the reason it is safe to carry and the condition to revisit it.

Out of scope: the security of third-party LLM backends you configure, and of the external binaries astria calls from `PATH` (`yt-dlp`, `ffmpeg`, `whisper-cli`, the `psql` used for `add --postgres`). How astria invokes them is in scope; their own bugs are not.
