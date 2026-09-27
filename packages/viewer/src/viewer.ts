// astria single-file graph viewer.
//
// Rendered from a precomputed layout (community bubbles on a sunflower
// spiral, member nodes fanning out around their community center — see
// crates/astria-napi/src/export_html.rs). No physics, no dependencies, no
// network: the whole page is inlined by the exporter, so it also works in
// sandboxed previewers.
//
// Interaction model (drill-down first):
//   - Default: one bubble per community. Click to expand a community into its
//     member nodes; click again to collapse.
//   - "All nodes": expand everything (level-of-detail labels).
//   - Click a member node: 1-hop neighborhood focus + details panel.
//   - Search: finds nodes by label/file, jumps to the match.
//
// Security rule: labels come from repo content / LLM output, so nothing is
// ever concatenated into markup — canvas text, or DOM textContent only.

interface DataNode {
  id: string;
  label: string;
  fileType: string;
  sourceFile: string;
  sourceLine: number | null;
  community: number | null;
  color: string;
  x: number;
  y: number;
  degree: number;
}

interface DataEdge {
  from: string;
  to: string;
}

interface DataCommunity {
  id: number | null;
  label: string;
  size: number;
  color: string;
  x: number;
  y: number;
}

interface DataHyperedge {
  id: string;
  label: string;
  nodes: string[];
}

interface GraphData {
  nodes: DataNode[];
  edges: DataEdge[];
  communities: DataCommunity[];
  hyperedges: DataHyperedge[];
  meta: { nodeCount: number; edgeCount: number; communityCount: number };
}

declare const DATA: GraphData;

type Ctx = CanvasRenderingContext2D;

const NONE_KEY = 'none';

interface Bubble {
  key: string;
  data: DataCommunity;
  members: DataNode[];
  radius: number;
}

interface View {
  cx: number;
  cy: number;
  scale: number;
}

const state = {
  view: { cx: 0, cy: 0, scale: 1 } as View,
  dpr: 1,
  width: 0,
  height: 0,
  expanded: new Set<string>(),
  allExpanded: false,
  hover: null as Bubble | DataNode | null,
  selected: null as DataNode | null,
  neighbors: new Set<string>(),
  dragging: false,
  moved: false,
  dragStart: { x: 0, y: 0, view: { cx: 0, cy: 0 } },
  query: '',
};

const MIN_SCALE = 0.02;
const MAX_SCALE = 8;
const NODE_RADIUS = 3.5;
const HIT_RADIUS = 8;
const LABEL_SCALE = 0.35; // member labels appear once zoomed in past this

let canvas: HTMLCanvasElement;
let ctx: Ctx;
let bubbles: Bubble[] = [];
let clickTimer: number | null = null;

// ---- data prep -------------------------------------------------------------

const nodeById = new Map<string, DataNode>();
const adjacency = new Map<string, string[]>();
const memberEdges: DataEdge[] = [];

function keyOf(community: number | null): string {
  return community === null ? NONE_KEY : 'c' + community;
}

function initData(): void {
  for (const n of DATA.nodes) {
    nodeById.set(n.id, n);
    adjacency.set(n.id, []);
  }
  for (const e of DATA.edges) {
    const a = adjacency.get(e.from);
    const b = adjacency.get(e.to);
    if (a) a.push(e.to);
    if (b) b.push(e.from);
    if (nodeById.has(e.from) && nodeById.has(e.to)) memberEdges.push(e);
  }
  const byKey = new Map<string, DataNode[]>();
  for (const n of DATA.nodes) {
    const k = keyOf(n.community);
    const list = byKey.get(k);
    if (list) list.push(n);
    else byKey.set(k, [n]);
  }
  for (const c of DATA.communities) {
    const members = byKey.get(keyOf(c.id)) || [];
    let radius = 20;
    for (const m of members) {
      const d = Math.hypot(m.x - c.x, m.y - c.y);
      if (d + 15 > radius) radius = d + 15;
    }
    const bubble: Bubble = { key: keyOf(c.id), data: c, members, radius };
    bubbles.push(bubble);
  }
  // Nodes without a community get a bubble of their own when the exporter
  // did not emit one (defensive; the exporter always emits it).
  const leftovers = byKey.get(NONE_KEY);
  if (leftovers && leftovers.length && !bubbles.some((b) => b.key === NONE_KEY)) {
    const cx = Math.min(...leftovers.map((n) => n.x));
    const cy = Math.min(...leftovers.map((n) => n.y));
    bubbles.push({
      key: NONE_KEY,
      data: { id: null, label: 'No community', size: leftovers.length, color: '#94a3b8', x: cx, y: cy },
      members: leftovers,
      radius: 30,
    });
  }
}

// ---- transform -------------------------------------------------------------

function toScreen(x: number, y: number): [number, number] {
  const v = state.view;
  return [(x - v.cx) * v.scale + state.width / 2, (y - v.cy) * v.scale + state.height / 2];
}

function fitBubbles(): void {
  fit(boundsOf(bubbles));
}

function boundsOf(bubbleList: Bubble[]): { minX: number; minY: number; maxX: number; maxY: number } {
  let minX = Infinity;
  let minY = Infinity;
  let maxX = -Infinity;
  let maxY = -Infinity;
  for (const b of bubbleList) {
    const r = b.radius;
    if (b.data.x - r < minX) minX = b.data.x - r;
    if (b.data.y - r < minY) minY = b.data.y - r;
    if (b.data.x + r > maxX) maxX = b.data.x + r;
    if (b.data.y + r > maxY) maxY = b.data.y + r;
  }
  if (!isFinite(minX)) {
    minX = -100;
    minY = -100;
    maxX = 100;
    maxY = 100;
  }
  return { minX, minY, maxX, maxY };
}

function fit(b: { minX: number; minY: number; maxX: number; maxY: number }, padding = 0.9): void {
  const bw = Math.max(b.maxX - b.minX, 1);
  const bh = Math.max(b.maxY - b.minY, 1);
  const scale = padding * Math.min(state.width / bw, state.height / bh);
  state.view.scale = clamp(scale, MIN_SCALE, MAX_SCALE);
  state.view.cx = (b.minX + b.maxX) / 2;
  state.view.cy = (b.minY + b.maxY) / 2;
}

function clamp(v: number, lo: number, hi: number): number {
  return v < lo ? lo : v > hi ? hi : v;
}

function isExpanded(key: string): boolean {
  return state.allExpanded || state.expanded.has(key);
}

function memberVisible(n: DataNode): boolean {
  return isExpanded(keyOf(n.community));
}

// ---- drawing ---------------------------------------------------------------

function draw(): void {
  ctx.setTransform(state.dpr, 0, 0, state.dpr, 0, 0);
  ctx.clearRect(0, 0, state.width, state.height);
  if (!DATA || !DATA.nodes.length) {
    ctx.fillStyle = '#e0e0e0';
    ctx.font = '14px system-ui, sans-serif';
    ctx.textAlign = 'center';
    ctx.fillText('The graph is empty — nothing to visualize.', state.width / 2, state.height / 2);
    return;
  }

  drawHulls();
  if (state.allExpanded || state.expanded.size) drawMemberEdges();
  else drawBubbleEdges();
  if (state.allExpanded) {
    drawAllMembers();
  } else {
    drawExpandedMembers();
    drawBubbles();
  }
  drawSelectionRing();
}

type Seg = [number, number, number, number];

function bubbleEdgesByBucket(): [number, Seg[]][] {
  const pairCount = new Map<string, number>();
  for (const e of DATA.edges) {
    const a = nodeById.get(e.from);
    const b = nodeById.get(e.to);
    if (!a || !b) continue;
    const ka = keyOf(a.community);
    const kb = keyOf(b.community);
    if (ka === kb) continue;
    const pk = ka < kb ? ka + '\u0000' + kb : kb + '\u0000' + ka;
    pairCount.set(pk, (pairCount.get(pk) || 0) + 1);
  }
  const buckets: [number, Seg[]][] = [
    [0.09, []],
    [0.18, []],
    [0.32, []],
    [0.55, []],
  ];
  for (const [pk, count] of pairCount) {
    const [ka, kb] = pk.split('\u0000');
    const ba = bubbles.find((b) => b.key === ka);
    const bb = bubbles.find((b) => b.key === kb);
    if (!ba || !bb || isExpanded(ka) || isExpanded(kb)) continue;
    const [sx, sy] = toScreen(ba.data.x, ba.data.y);
    const [tx, ty] = toScreen(bb.data.x, bb.data.y);
    const level = count > 64 ? 3 : count > 16 ? 2 : count > 4 ? 1 : 0;
    buckets[level][1].push([sx, sy, tx, ty]);
  }
  return buckets;
}

function drawBubbleEdges(): void {
  // Community-pair links, batched into alpha buckets so the whole edge set
  // costs a handful of strokes instead of one per pair.
  for (const [alpha, segs] of bubbleEdgesByBucket()) {
    if (!segs.length) continue;
    ctx.save();
    ctx.globalAlpha = alpha;
    ctx.strokeStyle = '#8b93a7';
    ctx.lineWidth = 1;
    ctx.beginPath();
    for (const [ax, ay, bx, by] of segs) {
      ctx.moveTo(ax, ay);
      ctx.lineTo(bx, by);
    }
    ctx.stroke();
    ctx.restore();
  }
}

function drawMemberEdges(): void {
  ctx.save();
  ctx.strokeStyle = '#7a86a0';
  ctx.lineWidth = 1;
  ctx.globalAlpha = state.selected ? 0.12 : 0.3;
  ctx.beginPath();
  const sel = state.selected;
  const highlight: Seg[] = [];
  for (const e of memberEdges) {
    const a = nodeById.get(e.from);
    const b = nodeById.get(e.to);
    if (!a || !b || !memberVisible(a) || !memberVisible(b)) continue;
    const [ax, ay] = toScreen(a.x, a.y);
    const [bx, by] = toScreen(b.x, b.y);
    if (sel && (e.from === sel.id || e.to === sel.id)) {
      highlight.push([ax, ay, bx, by]);
      continue;
    }
    ctx.moveTo(ax, ay);
    ctx.lineTo(bx, by);
  }
  ctx.stroke();
  ctx.restore();
  if (highlight.length) {
    ctx.save();
    ctx.globalAlpha = 0.85;
    ctx.strokeStyle = '#ffffff';
    ctx.lineWidth = 1.4;
    ctx.beginPath();
    for (const [ax, ay, bx, by] of highlight) {
      ctx.moveTo(ax, ay);
      ctx.lineTo(bx, by);
    }
    ctx.stroke();
    ctx.restore();
  }
}

function drawBubbles(): void {
  ctx.textAlign = 'center';
  const showLabels = state.view.scale >= 0.06;
  for (const b of bubbles) {
    if (isExpanded(b.key)) continue;
    const [sx, sy] = toScreen(b.data.x, b.data.y);
    const r = b.radius * state.view.scale;
    if (sx + r < 0 || sx - r > state.width || sy + r < 0 || sy - r > state.height) continue;
    ctx.save();
    ctx.globalAlpha = 0.16;
    ctx.fillStyle = b.data.color;
    ctx.beginPath();
    ctx.arc(sx, sy, r, 0, Math.PI * 2);
    ctx.fill();
    ctx.restore();
    ctx.save();
    ctx.globalAlpha = 0.9;
    ctx.strokeStyle = b.data.color;
    ctx.lineWidth = 1.6;
    ctx.beginPath();
    ctx.arc(sx, sy, r, 0, Math.PI * 2);
    ctx.stroke();
    ctx.restore();
    if (showLabels) {
      ctx.fillStyle = '#e0e0e0';
      ctx.font = '600 12px system-ui, sans-serif';
      ctx.fillText(b.data.label, sx, sy - r - 7);
    }
  }
}

function drawMemberBodies(n: DataNode, sx: number, sy: number): void {
  ctx.fillStyle = n.color;
  ctx.beginPath();
  ctx.arc(sx, sy, NODE_RADIUS, 0, Math.PI * 2);
  ctx.fill();
}

function drawExpandedMembers(): void {
  const v = state.view;
  const showLabels = v.scale >= LABEL_SCALE;
  const sel = state.selected;
  const dim = sel !== null;
  ctx.textAlign = 'center';
  for (const n of DATA.nodes) {
    if (!memberVisible(n)) continue;
    const [sx, sy] = toScreen(n.x, n.y);
    if (sx < -20 || sx > state.width + 20 || sy < -20 || sy > state.height + 20) continue;
    ctx.save();
    if (dim && !state.neighbors.has(n.id)) ctx.globalAlpha = 0.1;
    drawMemberBodies(n, sx, sy);
    if (showLabels || n === sel) {
      ctx.fillStyle = dim && !state.neighbors.has(n.id) ? 'rgba(224,224,224,0.25)' : '#cfd4e0';
      ctx.font = '11px system-ui, sans-serif';
      ctx.fillText(n.label, sx, sy - 8);
    }
    ctx.restore();
  }
}

function drawAllMembers(): void {
  drawExpandedMembers();
}

function drawSelectionRing(): void {
  const sel = state.selected;
  if (!sel) return;
  const [sx, sy] = toScreen(sel.x, sel.y);
  ctx.save();
  ctx.strokeStyle = '#ffffff';
  ctx.lineWidth = 1.6;
  ctx.beginPath();
  ctx.arc(sx, sy, NODE_RADIUS + 4, 0, Math.PI * 2);
  ctx.stroke();
  ctx.restore();
}

function drawHulls(): void {
  // Convex hulls for hyperedges (modules, cycles, ...) behind the members.
  for (const h of DATA.hyperedges || []) {
    if (h.nodes.length < 3) continue;
    const pts: [number, number][] = [];
    for (const id of h.nodes) {
      const n = nodeById.get(id);
      if (n && memberVisible(n)) pts.push(toScreen(n.x, n.y));
    }
    if (pts.length < 3) continue;
    const hull = convexHull(pts);
    const path = new Path2D();
    for (let i = 0; i < hull.length; i++) {
      const [x, y] = hull[i];
      if (i === 0) path.moveTo(x, y);
      else path.lineTo(x, y);
    }
    path.closePath();
    ctx.save();
    ctx.globalAlpha = 0.07;
    ctx.fillStyle = '#6366f1';
    ctx.fill(path);
    ctx.globalAlpha = 0.28;
    ctx.strokeStyle = '#6366f1';
    ctx.lineWidth = 1;
    ctx.stroke(path);
    ctx.restore();
  }
}

function convexHull(pts: [number, number][]): [number, number][] {
  const p = pts.slice().sort((a, b) => a[0] - b[0] || a[1] - b[1]);
  const cross = (o: [number, number], a: [number, number], b: [number, number]): number =>
    (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0]);
  const lower: [number, number][] = [];
  for (const pt of p) {
    while (lower.length >= 2 && cross(lower[lower.length - 2], lower[lower.length - 1], pt) <= 0)
      lower.pop();
    lower.push(pt);
  }
  const upper: [number, number][] = [];
  for (let i = p.length - 1; i >= 0; i--) {
    const pt = p[i];
    while (upper.length >= 2 && cross(upper[upper.length - 2], upper[upper.length - 1], pt) <= 0)
      upper.pop();
    upper.push(pt);
  }
  lower.pop();
  upper.pop();
  return lower.concat(upper);
}

// ---- hit testing & interaction ----------------------------------------------

function hitTest(sx: number, sy: number): Bubble | DataNode | null {
  if (state.allExpanded || state.expanded.size) {
    for (let i = DATA.nodes.length - 1; i >= 0; i--) {
      const n = DATA.nodes[i];
      if (!memberVisible(n)) continue;
      const [nx, ny] = toScreen(n.x, n.y);
      if ((sx - nx) * (sx - nx) + (sy - ny) * (sy - ny) <= HIT_RADIUS * HIT_RADIUS) return n;
    }
  }
  if (!state.allExpanded) {
    for (let i = bubbles.length - 1; i >= 0; i--) {
      const b = bubbles[i];
      if (isExpanded(b.key)) continue;
      const [bx, by] = toScreen(b.data.x, b.data.y);
      const r = b.radius * state.view.scale;
      if ((sx - bx) * (sx - bx) + (sy - by) * (sy - by) <= (r + 6) * (r + 6)) return b;
    }
  }
  return null;
}

function onPointerDown(e: PointerEvent): void {
  state.dragging = true;
  state.moved = false;
  state.dragStart = { x: e.clientX, y: e.clientY, view: { cx: state.view.cx, cy: state.view.cy } };
  canvas.setPointerCapture(e.pointerId);
}

function onPointerMove(e: PointerEvent): void {
  if (state.dragging) {
    const dx = e.clientX - state.dragStart.x;
    const dy = e.clientY - state.dragStart.y;
    if (Math.abs(dx) + Math.abs(dy) > 4) state.moved = true;
    state.view.cx = state.dragStart.view.cx - dx / state.view.scale;
    state.view.cy = state.dragStart.view.cy - dy / state.view.scale;
    state.hover = null;
    hideTooltip();
    draw();
    return;
  }
  const hit = hitTest(e.clientX - canvasRect().left, e.clientY - canvasRect().top);
  state.hover = hit;
  updateCursor();
  updateTooltip(e);
}

function canvasRect(): DOMRect {
  return canvas.getBoundingClientRect();
}

function onPointerUp(e: PointerEvent): void {
  const wasDrag = state.dragging;
  state.dragging = false;
  if (!wasDrag || state.moved) return;
  const sx = e.clientX - canvasRect().left;
  const sy = e.clientY - canvasRect().top;
  const hit = hitTest(sx, sy);
  if (!hit) {
    clearSelection();
    return;
  }
  // Defer the single-click action so a double-click (below) can cancel it.
  if (clickTimer !== null) window.clearTimeout(clickTimer);
  const pending = hit;
  clickTimer = window.setTimeout(() => {
    clickTimer = null;
    if ('members' in pending) toggleBubble(pending);
    else selectNode(pending);
  }, 260);
}

function onDblClick(e: MouseEvent): void {
  if (clickTimer !== null) {
    window.clearTimeout(clickTimer);
    clickTimer = null;
  }
  const hit = hitTest(e.clientX - canvasRect().left, e.clientY - canvasRect().top);
  if (hit && 'members' in hit) {
    // Double-clicking a bubble expands and centers it.
    if (!state.expanded.has(hit.key)) state.expanded.add(hit.key);
    centerOnBubble(hit);
  } else if (hit) {
    centerOnNode(hit);
  }
}

function centerOnBubble(b: Bubble): void {
  state.view.cx = b.data.x;
  state.view.cy = b.data.y;
  const target = clamp(Math.min(state.width / (b.radius * 2) * 0.85, 1.1), MIN_SCALE, MAX_SCALE);
  state.view.scale = Math.max(state.view.scale, Math.max(target, 0.5));
  clearSelection();
  updateStatus();
  draw();
}

function onWheel(e: WheelEvent): void {
  e.preventDefault();
  const rect = canvasRect();
  const sx = e.clientX - rect.left;
  const sy = e.clientY - rect.top;
  const scale = clamp(state.view.scale * Math.exp(-e.deltaY * 0.0012), MIN_SCALE, MAX_SCALE);
  // Keep the world point under the cursor fixed while scaling.
  const wx = state.view.cx + (sx - state.width / 2) / state.view.scale;
  const wy = state.view.cy + (sy - state.height / 2) / state.view.scale;
  state.view.cx = wx - (sx - state.width / 2) / scale;
  state.view.cy = wy - (sy - state.height / 2) / scale;
  state.view.scale = scale;
  draw();
}

function toggleBubble(b: Bubble): void {
  if (state.allExpanded) return;
  if (state.expanded.has(b.key)) {
    state.expanded.delete(b.key);
    clearSelection();
  } else {
    state.expanded.add(b.key);
  }
  updateStatus();
  draw();
}

function selectNode(n: DataNode): void {
  state.selected = n;
  state.neighbors.clear();
  for (const nb of adjacency.get(n.id) || []) state.neighbors.add(nb);
  showInfoForNode(n);
  updateStatus();
  draw();
}

function clearSelection(): void {
  state.selected = null;
  state.neighbors.clear();
  hideInfo();
  draw();
}

function centerOnNode(n: DataNode): void {
  if (!isExpanded(keyOf(n.community))) {
    state.expanded.add(keyOf(n.community));
  }
  state.view.cx = n.x;
  state.view.cy = n.y;
  state.view.scale = Math.max(state.view.scale, 1.2);
  selectNode(n);
}

// ---- tooltip & info -----------------------------------------------------------

const tooltip = document.createElement('div');
const info = document.createElement('div');

function tooltipText(hit: Bubble | DataNode): string {
  if ('members' in hit) {
    return hit.data.label + ' — ' + hit.members.length + ' nodes';
  }
  let text = hit.label + ' — ' + (hit.sourceFile || '?');
  if (hit.sourceLine !== null && hit.sourceLine !== undefined) text += ':' + hit.sourceLine;
  return text + ' · ' + hit.degree + ' connections';
}

function updateTooltip(e: PointerEvent): void {
  if (!state.hover) {
    hideTooltip();
    return;
  }
  tooltip.textContent = tooltipText(state.hover);
  tooltip.style.display = 'block';
  let x = e.clientX - canvasRect().left + 14;
  let y = e.clientY - canvasRect().top + 14;
  if (x + tooltip.offsetWidth > state.width - 8) x = e.clientX - canvasRect().left - tooltip.offsetWidth - 8;
  if (y + tooltip.offsetHeight > state.height - 8) y = e.clientY - canvasRect().top - tooltip.offsetHeight - 8;
  tooltip.style.left = x + 'px';
  tooltip.style.top = y + 'px';
}

function hideTooltip(): void {
  tooltip.style.display = 'none';
}

function updateCursor(): void {
  canvas.style.cursor = state.hover ? 'pointer' : 'grab';
}

function showInfoForNode(n: DataNode): void {
  info.textContent = '';
  const label = document.createElement('div');
  label.className = 'info-label';
  label.textContent = n.label;
  info.appendChild(label);
  const meta = document.createElement('div');
  meta.className = 'info-meta';
  meta.textContent =
    (n.sourceFile || '?') +
    (n.sourceLine !== null && n.sourceLine !== undefined ? ':' + n.sourceLine : '') +
    ' | ' +
    n.fileType +
    ' | ' +
    (n.community === null ? 'no community' : 'community ' + n.community) +
    ' | ' +
    n.degree +
    ' connections';
  info.appendChild(meta);
  const hint = document.createElement('div');
  hint.className = 'info-hint';
  hint.textContent = state.neighbors.size + ' direct neighbors · Esc or click empty space to clear';
  info.appendChild(hint);
  info.style.display = 'block';
}

function hideInfo(): void {
  info.style.display = 'none';
}

// ---- search -------------------------------------------------------------------

const searchInput = document.createElement('input');
const searchResults = document.createElement('div');
let searchEntries: { haystack: string; node: DataNode }[] = [];
let firstMatch: DataNode | null = null;

function onSearchInput(): void {
  state.query = searchInput.value.trim().toLowerCase();
  searchResults.textContent = '';
  firstMatch = null;
  if (!state.query) {
    searchResults.style.display = 'none';
    return;
  }
  const matches: DataNode[] = [];
  for (const e of searchEntries) {
    if (e.haystack.indexOf(state.query) !== -1) {
      matches.push(e.node);
      if (matches.length >= 24) break;
    }
  }
  if (!matches.length) {
    const empty = document.createElement('div');
    empty.className = 'search-empty';
    empty.textContent = 'No matches';
    searchResults.appendChild(empty);
  }
  for (const node of matches) {
    const row = document.createElement('div');
    row.className = 'search-row';
    row.textContent = node.label + ' — ' + (node.sourceFile || '?');
    if (!firstMatch) firstMatch = node;
    row.addEventListener('click', () => focusSearchMatch(node));
    searchResults.appendChild(row);
  }
  searchResults.style.display = 'block';
}

function focusSearchMatch(n: DataNode): void {
  searchInput.blur();
  searchResults.style.display = 'none';
  centerOnNode(n);
}

function onSearchKeydown(e: KeyboardEvent): void {
  if (e.key === 'Enter') {
    if (firstMatch) focusSearchMatch(firstMatch);
  } else if (e.key === 'Escape') {
    searchInput.value = '';
    onSearchInput();
    searchInput.blur();
  }
}

// ---- controls ------------------------------------------------------------------

function updateStatus(): void {
  const statusText = document.getElementById('astria-status-text');
  if (!statusText) return;
  const mode = state.allExpanded
    ? 'all ' + DATA.meta.nodeCount + ' nodes'
    : state.expanded.size
      ? 'overview + ' + state.expanded.size + ' expanded'
      : 'overview';
  statusText.textContent =
    mode +
    ' · ' +
    DATA.meta.communityCount +
    ' communities · ' +
    DATA.meta.nodeCount +
    ' nodes · ' +
    DATA.meta.edgeCount +
    ' edges';
}

function setAllExpanded(on: boolean): void {
  state.allExpanded = on;
  if (!on) {
    state.expanded.clear();
    fitBubbles();
  }
  clearSelection();
  updateStatus();
  updateModeButtons();
  draw();
}

function updateModeButtons(): void {
  const overview = document.getElementById('astria-mode-overview');
  const all = document.getElementById('astria-mode-all');
  if (!overview || !all) return;
  overview.classList.toggle('active', !state.allExpanded);
  all.classList.toggle('active', state.allExpanded);
}

function zoomBy(factor: number): void {
  // Zoom around the viewport center: the world point at the center stays put.
  state.view.scale = clamp(state.view.scale * factor, MIN_SCALE, MAX_SCALE);
  draw();
}

function fitView(): void {
  clearSelection();
  if (state.allExpanded) {
    const minX = Math.min(...DATA.nodes.map((n) => n.x));
    const minY = Math.min(...DATA.nodes.map((n) => n.y));
    const maxX = Math.max(...DATA.nodes.map((n) => n.x));
    const maxY = Math.max(...DATA.nodes.map((n) => n.y));
    fit({ minX, minY, maxX, maxY });
  } else {
    fitBubbles();
  }
  draw();
}

function buildControls(): void {
  const style = document.createElement('style');
  style.textContent = `
    html, body { margin: 0; padding: 0; overflow: hidden; background: #1a1a2e; }
    #astria-canvas { display: block; width: 100vw; height: 100vh; cursor: grab; }
    #astria-search { position: fixed; top: 12px; left: 12px; z-index: 100; width: 300px; }
    #astria-search input {
      width: 100%; box-sizing: border-box; padding: 8px 12px; border: 1px solid #444;
      border-radius: 6px; background: #16213e; color: #e0e0e0; font: 14px system-ui, sans-serif;
      outline: none;
    }
    #astria-search input:focus { border-color: #6366f1; }
    #astria-search input::placeholder { color: #888; }
    #astria-results {
      display: none; margin-top: 4px; border: 1px solid #333; border-radius: 6px;
      background: #16213e; max-height: 320px; overflow-y: auto; font: 13px system-ui, sans-serif;
    }
    #astria-results .search-row { padding: 6px 10px; color: #cfd4e0; cursor: pointer; }
    #astria-results .search-row:hover { background: #1f2c53; }
    #astria-results .search-empty { padding: 6px 10px; color: #888; }
    #astria-tooltip {
      position: fixed; display: none; z-index: 120; max-width: 380px; padding: 6px 10px;
      background: #16213e; border: 1px solid #333; border-radius: 6px; color: #cfd4e0;
      font: 12px system-ui, sans-serif; pointer-events: none;
    }
    #astria-info {
      position: fixed; bottom: 12px; left: 12px; z-index: 100; display: none; max-width: 420px;
      background: #16213e; border: 1px solid #333; border-radius: 6px; padding: 10px 14px;
      font: 13px system-ui, sans-serif;
    }
    #astria-info .info-label { font-weight: 600; font-size: 15px; color: #e0e0e0; margin-bottom: 4px; word-break: break-word; }
    #astria-info .info-meta { color: #aaa; }
    #astria-info .info-hint { color: #666; margin-top: 6px; }
    #astria-zoombar {
      position: fixed; right: 12px; top: 50%; transform: translateY(-50%); z-index: 100;
      display: flex; flex-direction: column; gap: 6px;
    }
    #astria-zoombar button {
      width: 34px; height: 34px; border: 1px solid #444; border-radius: 6px; background: #16213e;
      color: #e0e0e0; font: 16px system-ui, sans-serif; cursor: pointer;
    }
    #astria-zoombar button:hover { background: #1f2c53; }
    #astria-status {
      position: fixed; bottom: 12px; right: 12px; z-index: 100; display: flex; align-items: center;
      gap: 8px; background: #16213e; border: 1px solid #333; border-radius: 6px; padding: 8px 12px;
      font: 12px system-ui, sans-serif; color: #aaa;
    }
    #astria-status .mode-btn {
      border: 1px solid #444; border-radius: 4px; background: #0f3460; color: #e0e0e0;
      padding: 4px 10px; font: 12px system-ui, sans-serif; cursor: pointer;
    }
    #astria-status .mode-btn.active { background: #2d5aa0; border-color: #6366f1; }
    #astria-hint {
      position: fixed; bottom: 12px; left: 50%; transform: translateX(-50%); z-index: 90;
      color: #777; font: 12px system-ui, sans-serif; pointer-events: none;
    }
  `;
  document.head.appendChild(style);

  const searchBox = document.createElement('div');
  searchBox.id = 'astria-search';
  searchInput.type = 'text';
  searchInput.placeholder = 'Search nodes...  (/)';
  searchInput.setAttribute('aria-label', 'Search graph nodes');
  searchBox.appendChild(searchInput);
  searchResults.id = 'astria-results';
  searchResults.setAttribute('role', 'listbox');
  searchBox.appendChild(searchResults);

  tooltip.id = 'astria-tooltip';
  info.id = 'astria-info';

  const zoomBar = document.createElement('div');
  zoomBar.id = 'astria-zoombar';
  for (const [label, fn, title] of [
    ['+', () => zoomBy(1.35), 'Zoom in'],
    ['\u2212', () => zoomBy(1 / 1.35), 'Zoom out'],
    ['\u229e', fitView, 'Fit view (0)'],
  ] as [string, () => void, string][]) {
    const btn = document.createElement('button');
    btn.textContent = label;
    btn.title = title;
    btn.addEventListener('click', fn);
    zoomBar.appendChild(btn);
  }

  const status = document.createElement('div');
  status.id = 'astria-status';
  const overviewBtn = document.createElement('button');
  overviewBtn.id = 'astria-mode-overview';
  overviewBtn.className = 'mode-btn';
  overviewBtn.textContent = 'Overview';
  overviewBtn.title = 'Collapse all communities back into bubbles';
  overviewBtn.addEventListener('click', () => setAllExpanded(false));
  const allBtn = document.createElement('button');
  allBtn.id = 'astria-mode-all';
  allBtn.className = 'mode-btn';
  allBtn.textContent = 'All nodes';
  allBtn.title = 'Expand every community into its member nodes';
  allBtn.addEventListener('click', () => setAllExpanded(true));
  const statusText = document.createElement('span');
  statusText.id = 'astria-status-text';
  status.appendChild(overviewBtn);
  status.appendChild(allBtn);
  status.appendChild(statusText);

  const hint = document.createElement('div');
  hint.id = 'astria-hint';
  hint.textContent = 'click a community to expand it · click a node for its neighbors · scroll to zoom · drag to pan';

  document.body.append(canvas, searchBox, tooltip, info, zoomBar, status, hint);
}

function onResize(): void {
  state.dpr = Math.min(window.devicePixelRatio || 1, 2);
  state.width = window.innerWidth;
  state.height = window.innerHeight;
  canvas.width = Math.round(state.width * state.dpr);
  canvas.height = Math.round(state.height * state.dpr);
  draw();
}

function onKeydown(e: KeyboardEvent): void {
  const target = e.target as HTMLElement;
  // Inputs handle their own keys (Enter / Escape for search).
  if (target && (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA')) return;
  if (e.key === '/') {
    e.preventDefault();
    searchInput.focus();
  } else if (e.key === 'Escape') {
    clearSelection();
    if (state.query) {
      searchInput.value = '';
      onSearchInput();
    }
  } else if (e.key === '+') {
    zoomBy(1.35);
  } else if (e.key === '-') {
    zoomBy(1 / 1.35);
  } else if (e.key === '0') {
    fitView();
  }
}

function main(): void {
  canvas = document.createElement('canvas');
  canvas.id = 'astria-canvas';
  ctx = canvas.getContext('2d') as Ctx;
  if (!ctx || !DATA || !DATA.nodes) {
    document.body.appendChild(canvas);
    return;
  }
  buildControls();
  initData();
  searchEntries = DATA.nodes.map((n) => ({
    haystack: (n.label + ' ' + (n.sourceFile || '')).toLowerCase(),
    node: n,
  }));
  searchInput.addEventListener('input', onSearchInput);
  searchInput.addEventListener('keydown', onSearchKeydown);
  canvas.addEventListener('pointerdown', onPointerDown);
  canvas.addEventListener('pointermove', onPointerMove);
  canvas.addEventListener('pointerup', onPointerUp);
  canvas.addEventListener('pointercancel', () => {
    state.dragging = false;
  });
  canvas.addEventListener('wheel', onWheel, { passive: false });
  canvas.addEventListener('dblclick', onDblClick);
  window.addEventListener('resize', onResize);
  window.addEventListener('keydown', onKeydown);
  onResize();
  fitBubbles();
  updateStatus();
  updateModeButtons();
  draw();
}

main();