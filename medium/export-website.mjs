import {
  cpSync,
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  writeFileSync,
} from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { execFileSync } from "node:child_process";

// The article drafts and saved evidence in this directory remain the source.
const source = path.dirname(fileURLToPath(import.meta.url));
if (!process.argv[2])
  throw new Error(
    "Usage: node medium/export-website.mjs <nodesify-landing-path>",
  );
const target = path.resolve(process.argv[2]);
if (!existsSync(path.join(target, "src/content.config.ts"))) {
  throw new Error("Destination must be the Nodesify landing repository.");
}

const articles = [
  {
    file: "2026-10-give-your-coding-agent-a-map.md",
    slug: "astria-codebase-knowledge-graph-for-coding-agents",
    seoTitle: "Astria: Code Graphs for Coding Agents",
    description:
      "Use Astria to find code, inspect relationship evidence, and check potential change impact. A practical guide from its creators at Nodesify.",
    image: "00-relationship-map.png",
    imageAlt:
      "Code graph showing containment, a resolved call, and an unresolved target",
    featured: true,
  },
  {
    file: "2026-10-your-code-graph-can-invent-dependencies.md",
    slug: "astria-phantom-dependencies-graph-evidence",
    seoTitle: "Code Graph Bugs: Phantom Dependencies",
    description:
      "How Nodesify fixed false ownership in Astria, removed 743 phantom dependencies, and learned why retrieval scores cannot certify graph correctness.",
    image: "03-phantom-dependency.png",
    imageAlt:
      "False ownership fabricates a file dependency; clearing ownership preserves uncertainty",
    featured: false,
  },
  {
    file: "2026-10-how-we-evaluate-code-retrieval-tools.md",
    slug: "astria-code-retrieval-evaluation-2026",
    seoTitle: "Astria vs Graphify: Code Retrieval Evaluation",
    description:
      "Nodesify explains its October 2026 Astria–Graphify comparison: retrieval metrics, token budgets, build time, reproducible evidence, and limitations.",
    image: "04-evaluation-flow.png",
    imageAlt:
      "Paired retrieval evaluation separates raw budget compliance from scores after clipping",
    featured: false,
  },
];
const evidenceUrl = "/downloads/astria/2026-10-06";
const quote = (value) => JSON.stringify(value);
const blogDir = path.join(target, "src/content/blog");
const imageDir = path.join(target, "public/images/blog/astria");
const assetDir = path.join(target, "src/assets/blog");
const evidenceDir = path.join(target, "public", evidenceUrl.slice(1));
for (const dir of [blogDir, imageDir, assetDir, evidenceDir])
  mkdirSync(dir, { recursive: true });

for (const file of readdirSync(path.join(source, "images")).filter((file) =>
  /^0[0-4]-.*\.png$/.test(file),
)) {
  cpSync(
    path.join(source, "images", file),
    path.join(imageDir, `astria-${file}`),
  );
  cpSync(
    path.join(source, "images", file),
    path.join(assetDir, `astria-${file}`),
  );
}

for (const [index, article] of articles.entries()) {
  const original = readFileSync(
    path.join(source, article.file),
    "utf8",
  ).replace(/\r\n/g, "\n");
  const title = original.match(/^# (.+)$/m)?.[1];
  if (!title) throw new Error(`Missing title: ${article.file}`);
  let body = original
    .replace(/<!--[\s\S]*?-->\s*/g, "")
    .replace(/^# .+\n\n/, "")
    .replace(/^## .+\n\n/, "")
    .replace(/^\*By .+\n\n/m, "");
  for (const companion of articles) {
    body = body.replaceAll(
      `](${companion.file})`,
      `](/blog/${companion.slug})`,
    );
  }
  body = body.replace(
    /!\[([^\]]*)\]\(images\/(0[0-4]-[^)]+\.png)\)/g,
    (_, alt, file) => {
      const png = readFileSync(path.join(source, "images", file));
      const width = png.readUInt32BE(16);
      const height = png.readUInt32BE(20);
      const escapedAlt = alt
        .replaceAll("&", "&amp;")
        .replaceAll('"', "&quot;")
        .replaceAll("<", "&lt;")
        .replaceAll(">", "&gt;");
      return `<img src="/images/blog/astria/astria-${file}" alt="${escapedAlt}" width="${width}" height="${height}" loading="lazy" decoding="async" />`;
    },
  );
  body = body.replace(
    /\[([^\]]+)\]\(evidence\/2026-10-06\/README\.md\)/g,
    `[$1 (ZIP)](${evidenceUrl}/evidence.zip)`,
  );
  body = body.replace(
    /\]\(evidence\/2026-10-06\/([^)]*)\)/g,
    `](${evidenceUrl}/$1)`,
  );
  body = body
    .replace(
      "Follow Nodesify on Medium for the companion articles on graph correctness and evaluation.",
      "Read the companion articles below for graph correctness and evaluation, or explore more [Nodesify engineering articles](/blog/category/engineering).",
    )
    .replace(
      "Follow Nodesify on Medium for more engineering notes from the project.",
      "Explore more [Nodesify engineering articles](/blog/category/engineering) and the companion articles below.",
    )
    .replace(
      "Follow Nodesify on Medium for future evaluation reports that distinguish measured results from remaining questions.",
      "Explore more [Nodesify engineering articles](/blog/category/engineering) for practical implementation and evaluation work.",
    );
  const readingTime = `${Math.ceil(body.split(/\s+/).length / 220)} min read`;
  const fields = {
    title,
    seoTitle: article.seoTitle,
    description: article.description,
    category: "Engineering",
    author: "Nodesify",
    authorTitle: "Astria Engineering Team",
    authorBio:
      "Nodesify builds custom software and developer tools. We create and maintain Astria, a local codebase knowledge graph for coding agents.",
    publishedDate: "2026-10-06",
    readingTime,
    featured: article.featured,
    highlight: false,
    order: index,
    active: true,
    image: `/images/blog/astria/astria-${article.image}`,
    imageAlt: article.imageAlt,
    imageFit: "contain",
  };
  const frontmatter = Object.entries(fields)
    .map(
      ([key, value]) =>
        `${key}: ${typeof value === "string" && key !== "publishedDate" ? quote(value) : value}`,
    )
    .join("\n");
  writeFileSync(
    path.join(blogDir, `${article.slug}.md`),
    `---\n${frontmatter}\n---\n\n${body.trim()}\n`,
  );
}

cpSync(path.join(source, "evidence/2026-10-06"), evidenceDir, {
  recursive: true,
  filter: (file) => path.basename(file) !== "config.local.json",
});
const evidenceReadme = readFileSync(path.join(evidenceDir, "README.md"), "utf8")
  .replace(
    "This package supports the Medium article",
    "This package supports the Nodesify article",
  )
  .replace(
    "These files are ready for publication but are currently local. Upload them together to a public repository or release attachment, preserving this directory layout, then replace the article's relative evidence links with the actual public URLs. Do not describe this package as public until it is accessible.",
    "This package is distributed with the Nodesify website article. Keep its directory layout intact when extracting it. The reproduction instructions use this package at `medium/evidence/2026-10-06/` within a pinned Astria source checkout.",
  );
writeFileSync(path.join(evidenceDir, "README.md"), evidenceReadme);
// Use the platform archive utility instead of implementing a ZIP writer.
const archive = path.join(evidenceDir, "evidence.zip");
const files = readdirSync(evidenceDir).filter(
  (file) => file !== "evidence.zip",
);
if (process.platform === "win32") {
  const psQuote = (value) => `'${value.replaceAll("'", "''")}'`;
  execFileSync("powershell.exe", [
    "-NoProfile",
    "-Command",
    `Compress-Archive -LiteralPath ${files.map((file) => psQuote(path.join(evidenceDir, file))).join(",")} -DestinationPath ${psQuote(archive)} -Force`,
  ]);
} else {
  execFileSync("zip", ["-q", "-r", archive, ...files], { cwd: evidenceDir });
}
console.log(
  `Exported three articles, five diagrams, and evidence to ${target}`,
);
