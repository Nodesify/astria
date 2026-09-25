"use strict";
// PR impact dashboard: maps open PRs' changed files onto the knowledge graph
// (communities touched, node blast radius) and flags merge-order risk.
// Ported from upstream graphify v8 prs.py. Requires the `gh` CLI (read-only).
Object.defineProperty(exports, "__esModule", { value: true });
exports.prsCommand = prsCommand;
const child_process_1 = require("child_process");
const fs_1 = require("fs");
const path_1 = require("path");
const DIM = '\x1b[2m';
const BOLD = '\x1b[1m';
const YELLOW = '\x1b[33m';
const CYAN = '\x1b[36m';
const RESET = '\x1b[0m';
function ghAvailable() {
    try {
        (0, child_process_1.execFileSync)('gh', ['--version'], { stdio: 'pipe' });
        return true;
    }
    catch {
        return false;
    }
}
function listOpenPrs(limit) {
    const out = (0, child_process_1.execFileSync)('gh', ['pr', 'list', '--limit', String(limit), '--json', 'number,title,headRefName,baseRefName,isDraft'], { encoding: 'utf-8', stdio: ['pipe', 'pipe', 'pipe'] });
    return JSON.parse(out);
}
function prFiles(number) {
    if (!Number.isInteger(number) || number <= 0)
        return [];
    try {
        const out = (0, child_process_1.execFileSync)('gh', ['pr', 'view', String(number), '--json', 'files'], { encoding: 'utf-8', stdio: ['pipe', 'pipe', 'pipe'] });
        const parsed = JSON.parse(out);
        return (parsed.files || []).map((f) => f.path);
    }
    catch {
        return [];
    }
}
function loadGraphNodes(graphRoot) {
    const graphJson = (0, path_1.join)(graphRoot, '.graphify', 'graph.json');
    if (!(0, fs_1.existsSync)(graphJson)) {
        throw new Error(`no graph found at ${graphJson} — run 'nodesify-graphify run ${graphRoot}' first`);
    }
    const graph = JSON.parse((0, fs_1.readFileSync)(graphJson, 'utf-8'));
    return (graph.nodes || []).map((n) => ({
        source_file: String(n.source_file || '').replace(/\\/g, '/'),
        community: n.community ?? null,
    }));
}
function computeImpact(pr, files, nodes) {
    const normalized = files.map((f) => f.replace(/\\/g, '/'));
    const impact = {
        number: pr.number,
        title: pr.title,
        draft: pr.isDraft,
        files,
        nodes: new Set(),
        communities: new Map(),
    };
    for (const node of nodes) {
        if (!node.source_file)
            continue;
        const hit = normalized.some((f) => node.source_file === f || node.source_file.endsWith('/' + f));
        if (hit) {
            impact.nodes.add(node.source_file);
            if (node.community !== null) {
                impact.communities.set(node.community, (impact.communities.get(node.community) || 0) + 1);
            }
        }
    }
    return impact;
}
function printTable(impacts) {
    console.log(`${BOLD}Open pull requests mapped onto the knowledge graph${RESET}\n`);
    const numW = String(Math.max(...impacts.map((i) => i.number), 0)).length;
    for (const pr of impacts) {
        const draft = pr.draft ? `${DIM}[draft]${RESET} ` : '';
        const top = [...pr.communities.entries()].sort((a, b) => b[1] - a[1]).slice(0, 4);
        const commStr = top.map(([c, n]) => `${CYAN}${c}${RESET}:${n}`).join(' ') || '—';
        const symbols = [...pr.communities.values()].reduce((a, b) => a + b, 0);
        console.log(`${DIM}#${String(pr.number).padStart(numW)}${RESET} ${draft}${pr.title}`);
        console.log(`     ${DIM}${pr.files.length} file(s) · ${symbols} graph symbols · communities: ${commStr}${RESET}`);
    }
}
function printConflicts(impacts) {
    console.log(`\n${BOLD}Merge-order risk (shared communities)${RESET}\n`);
    let found = false;
    for (let i = 0; i < impacts.length; i++) {
        for (let j = i + 1; j < impacts.length; j++) {
            const shared = [...impacts[i].communities.keys()].filter((c) => impacts[j].communities.has(c));
            if (shared.length > 0) {
                found = true;
                console.log(`${YELLOW}#${impacts[i].number} ↔ #${impacts[j].number}${RESET} share ${shared.length} communit${shared.length === 1 ? 'y' : 'ies'}: ${shared.slice(0, 8).join(', ')}${shared.length > 8 ? ' …' : ''}`);
            }
        }
    }
    if (!found)
        console.log('No shared communities between open PRs.');
}
async function prsCommand(count, opts) {
    const limit = parseInt(count, 10) || 20;
    try {
        if (!ghAvailable()) {
            console.error('Error: the GitHub CLI (gh) is required for this command — https://cli.github.com');
            process.exitCode = 1;
            return;
        }
        let prs;
        try {
            prs = listOpenPrs(limit);
        }
        catch (e) {
            console.error(`Error: gh could not list PRs (not a git repo / not authenticated?): ${e.message || e}`);
            process.exitCode = 1;
            return;
        }
        if (prs.length === 0) {
            console.log('No open pull requests.');
            return;
        }
        const nodes = loadGraphNodes(opts.graph);
        const impacts = prs.map((pr) => computeImpact(pr, prFiles(pr.number), nodes));
        printTable(impacts);
        if (opts.conflicts)
            printConflicts(impacts);
    }
    catch (e) {
        console.error(`Error: ${e.message || e}`);
        process.exitCode = 1;
    }
}
//# sourceMappingURL=prs.js.map