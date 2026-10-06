# Medium article series

Three standalone drafts replace the original long article. Each explains its own question and links to deeper material; readers do not need to read them in order.

| Draft | Purpose | Approximate length | Images |
| --- | --- | ---: | ---: |
| [Give Your Coding Agent a Map](2026-10-give-your-coding-agent-a-map.md) | Practical introduction, worked source investigation, agent setup, and related approaches | About 1,500 words | 3 |
| [Your Code Graph Can Invent Dependencies](2026-10-your-code-graph-can-invent-dependencies.md) | Phantom-dependency defect, code explanation, evidence classes, and structural correctness | About 1,400 words | 1 |
| [How We Evaluate Code Retrieval Tools](2026-10-how-we-evaluate-code-retrieval-tools.md) | Paired protocol, reproduction, retrieval metrics, budgets, build cost, and limits | About 1,500 words | 1 |

## Publishing workflow

Publish the website editions first, then import those URLs into Medium. The Markdown drafts here remain the editorial source; `export-website.mjs` adds website metadata and rewrites platform-specific links and invitations without changing the technical claims.

```sh
node medium/export-website.mjs PATH_TO_NODESIFY_LANDING
```

The target must be the Nodesify landing repository. The exporter prepares three Engineering articles, five diagram assets, and a downloadable evidence ZIP. It uses PowerShell's archive utility on Windows or `zip` on other systems. Deployment is a separate step. See the landing repository's `docs/astria-article-publication.md` for release checks.

| Website edition | Canonical URL after deployment |
| --- | --- |
| Introduction | `https://nodesify.com/blog/astria-codebase-knowledge-graph-for-coding-agents` |
| Correctness | `https://nodesify.com/blog/astria-phantom-dependencies-graph-evidence` |
| Evaluation | `https://nodesify.com/blog/astria-code-retrieval-evaluation-2026` |

The evidence ZIP will be served at `https://nodesify.com/downloads/astria/2026-10-06/evidence.zip`. These are prepared destinations; verify them after deployment before treating them as public. Set each Medium story's canonical URL to its website edition, using [Medium's import tool](https://help.medium.com/hc/en-us/articles/214550207-Importing-a-post-to-Medium) or [canonical-link settings](https://help.medium.com/hc/en-us/articles/360033930293-Set-a-canonical-link).

1. Create a Medium draft for each article. Remove all HTML comments before publishing, including topic metadata and `PUBLISH` notes; this README is not article content. Enter topics through Medium's publishing interface.
2. Paste section by section and check title, subtitle, headings, links, and paragraph breaks. Put every fenced block in a Medium code block. Keep commands copyable rather than replacing them with screenshots.
3. Upload the PNGs at the image markers in the order listed below. Local Markdown paths do not upload images. Use the bracket text as alt text and the italic sentence below each image as its caption.
4. Review the preview on desktop and mobile. The diagrams explain different concepts; do not combine them into one dense image. Check code wrapping, particularly the long authentication query.
5. Replace relative links to companion `.md` drafts with their actual published Medium URLs. If a companion is not live yet, omit its local link from the published article and add it once available. Do not publish local filesystem links or invent destination URLs. All three articles already include public documentation links, so they stand alone during a staged release.
6. Recheck the package version and public source links before release. Claims describe astria 1.1.0 and the October 6, 2026 evaluation. If the evidence changes, update the relevant text rather than silently treating historical results as current.
7. Publish the [evaluation evidence package](evidence/2026-10-06/README.md), then replace every article link beginning `evidence/` with its actual public URL. Publish the report, summaries, raw-results archive, configuration, and inputs together. Local files are prepared here; their presence does not mean they are publicly accessible.
8. Keep citations next to numerical and implementation claims, with the references list for navigation. Preserve commit-specific source links. The correctness story's numbers are supported by maintainer release records; its separate raw before/after experiment is not bundled with the paired evaluation. Do not present the paired archive as evidence for that different experiment.
9. Retain the affiliation disclosures. The team byline is usable as written; if a named engineer authors the Medium post, use their approved name and a brief accurate role rather than inventing a biography.

Suggested reader-facing release order: introduction, correctness story, evaluation. If all cross-links must be live at the introduction's release, publish the companion articles first and link them from the introduction.

## Official account and brand attribution

Publish this series from Nodesify's official account. [Prepared account-profile copy](official-account-profile.md) includes a display name, short bio, About text, and company/project links for manual use. No live account changes have been made.

Each article now identifies Nodesify in the byline and introduction, ends with an invitation relevant to its subject, and includes a consistent company footer. Retain the maintainer disclosure and the distinction between reported measurements and general claims. The company link is `https://nodesify.com`, as used in Astria's README and the company site; the project link is `https://github.com/Nodesify/astria`.

The closing invitations ask readers to try Astria, inspect its evidence, or reproduce the evaluation. Keep those useful next steps instead of adding unrelated sales claims. “Follow Nodesify on Medium” assumes publication from the official account; if publishing elsewhere, replace that phrase with the verified official profile link before release.

## Suggested Medium topics

Medium allows up to five topics per article; see [its topic guidance](https://help.medium.com/hc/en-us/articles/214741038-Using-topics). These are content-based suggestions, not measured popularity or traffic promises. Choose matching available topics in Medium's interface; use fewer if no relevant match exists. The same suggestions appear in each draft's opening HTML comment.

| Draft | Suggested topics |
| --- | --- |
| Introduction | Programming; Artificial Intelligence; Developer Tools; Knowledge Graphs; Software Engineering |
| Correctness story | Software Engineering; Debugging; Knowledge Graphs; Static Analysis; Programming |
| Evaluation | Information Retrieval; Benchmarking; Artificial Intelligence; Developer Tools; Software Engineering |

## Citations and evidence

The drafts use inline links beside claims plus short references sections. Related-work links describe other approaches; they are not independent validation of Astria's results. Graphify's confidence labels and benchmark protocols should not be treated as equivalent to Astria's.

The [evidence package](evidence/2026-10-06/README.md) was exported from the saved October 6 run without rerunning retrieval. Its original report, raw-results archive, JSON/CSV summaries, and copied question inputs make the measurements inspectable. The configuration is a template requiring local paths and built dependencies. Instructions support repeating the protocol; they do not promise identical timing, scores, or binaries.

Before release, open the public evidence links in a signed-out browser. Check that the compressed raw results download successfully and that the public package preserves the relative paths used by its README. For the correctness story, publish separate pre-fix/post-fix artifacts if available and add their links beside the numerical claim; otherwise keep the current maintainer-report attribution.

## Image manifest

The PNGs are exported at 2x resolution from their editable HTML/SVG sources in `images/src/`. They are explanatory diagrams, not direct terminal screenshots or measurements of an agent's task performance.

| Article | Position | PNG | Meaning |
| --- | --- | --- | --- |
| Introduction | What a map adds | [00-relationship-map.png](images/00-relationship-map.png) | Containment, a resolved call, and an unresolved name |
| Introduction | One task, two response paths | [01-request-path.png](images/01-request-path.png) | Early rejection before request-body reading, plus the later response path |
| Introduction | Make the workflow available | [02-workflow.png](images/02-workflow.png) | Ask → inspect evidence → read source → check dependents → edit → refresh |
| Correctness story | Borrowed location explanation | [03-phantom-dependency.png](images/03-phantom-dependency.png) | False ownership fabricates a file dependency; removing ownership preserves uncertainty |
| Evaluation | What was held constant | [04-evaluation-flow.png](images/04-evaluation-flow.png) | Raw compliance versus scoring after shared clipping |

Use `00-relationship-map.png` as the introductory article's featured image and `03-phantom-dependency.png` for the correctness article. The evaluation diagram is suitable for that article's preview. Check thumbnail crops in Medium before saving.

## Supporting output

The seven earlier images and their sources have moved to `images/reference/` and `images/src/reference/`. They remain available in [the output reference](reference-output.md), outside the article reading flow. They describe a historical development graph, not the paired benchmark corpus.

The benchmark draft retains the corrected totals: eight corpus/split conditions from four repositories; astria leads file MRR in seven, Graphify builds faster in seven; 85 responses at each budget per tool. Evaluation sets were previously exercised; the separately reserved Requests and Commander questions were not used. The 244.6× full-read ratio is explicitly separated from exact-token measurements and total task savings.
