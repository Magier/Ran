# ELK Layout Migration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the `cytoscape-fcose` spring-physics layout with ELK's `layered` algorithm to get a deterministic, left-to-right attack-chain layout with tight namespace compound grouping and intuitive spatial conventions.

**Architecture:** Top-level layout uses ELK `layered` (direction RIGHT) so the attack chain reads left-to-right naturally - Ran/C2 on the left, cloud/infra on the right. Within Namespace compound nodes, ELK `stress` keeps pod/serviceaccount/container tightly clustered. Existing node positions are passed as `elk.position` hints with `elk.interactiveLayout: true` so subsequent graph updates don't rearrange nodes that already have a settled position.

**Tech Stack:** `elkjs`, `cytoscape-elk`, existing Cytoscape + `cytoscape-expand-collapse` stack (unchanged), Svelte 5, TypeScript.

---

## File Map

| File | Change | Responsibility |
|---|---|---|
| `frontend/package.json` | modify | add `elkjs`, `cytoscape-elk` deps |
| `frontend/src/routes/components/elk_layout.ts` | **create** | ELK options builder - layer assignment, edge filtering, per-node options |
| `frontend/src/routes/components/graph_style.ts` | modify | remove `createLayout` / `layout` exports; keep all CSS styles |
| `frontend/src/routes/components/graph.svelte` | modify | swap fcose for elk, remove lock/unlock, pass positions to layout |

---

## Task 1: Install dependencies

**Files:**
- Modify: `frontend/package.json`

- [ ] **Step 1: Install packages**

```bash
cd frontend
pnpm add elkjs cytoscape-elk
```

- [ ] **Step 2: Verify resolution**

```bash
pnpm ls elkjs cytoscape-elk
```

Expected output: both packages listed at a resolved version (cytoscape-elk ≥ 1.2.0, elkjs ≥ 0.9.0).

- [ ] **Step 3: Commit**

```bash
git add frontend/package.json frontend/pnpm-lock.yaml
git commit -m "chore: add elkjs and cytoscape-elk"
```

---

## Task 2: Create `elk_layout.ts`

**Files:**
- Create: `frontend/src/routes/components/elk_layout.ts`

This file owns everything layout-specific. `graph_style.ts` keeps visual styles only.

- [ ] **Step 1: Write the file**

```typescript
import type cytoscape from 'cytoscape';
import { INFORMATIONAL_EDGES } from './edge_categories';

/**
 * Maps node kind → ELK layer index (lower = further left = earlier in attack chain).
 * Unmapped kinds land wherever ELK's topological sort places them.
 */
export const NODE_LAYER: Record<string, number> = {
  // Attack origin
  C2: 0,
  // External machines / adversary infrastructure
  Adversary: 1,
  System: 1,
  // C2 channels
  Listener: 2,
  Session: 2,
  // Cluster entry points
  Ingress: 3,
  Service: 3,
  // Workloads
  Pod: 4,
  Container: 4,
  MicroService: 4,
  AbstractWorkload: 4,
  Deployment: 4,
  ReplicaSet: 4,
  StatefulSet: 4,
  DaemonSet: 4,
  Job: 4,
  CronJob: 4,
  // RBAC / k8s resources (tend to be used BY workloads, so one step right)
  ServiceAccount: 5,
  Role: 5,
  ClusterRole: 5,
  RoleBinding: 5,
  ClusterRoleBinding: 5,
  User: 5,
  Group: 5,
  ConfigMap: 5,
  Secret: 5,
  Volume: 5,
  // Control plane
  KubeApiServer: 6,
  ControlPlane: 6,
  // Infrastructure nodes (also pushed south via priority)
  Node: 7,
  ClusterNode: 7,
  // Cloud resources
  GCPBucket: 8,
  GCPServiceAccount: 8,
  GCPServiceAccountToken: 8,
  MetadataServer: 8,
  GCPMetadataServer: 8,
};

/**
 * Returns true for edges that should participate in layout.
 * Informational edges (runs-on, owns, contains, etc.) are excluded -
 * they add noise without contributing to the attack-chain shape.
 */
function isLayoutEdge(edge: cytoscape.EdgeSingular): boolean {
  return !INFORMATIONAL_EDGES.has(edge.data('name'));
}

/**
 * Serialise a position for the elk.position layout option.
 */
function elkPos(x: number, y: number): string {
  return `(${Math.round(x)}, ${Math.round(y)})`;
}

/**
 * Builds a Cytoscape layout options object that drives ELK.
 *
 * @param positions - Saved positions from sessionStorage; used as hints for
 *                    elk.interactiveLayout so existing nodes don't move.
 */
export function createElkLayout(
  positions: Record<string, { x: number; y: number }> = {}
): cytoscape.LayoutOptions & Record<string, unknown> {
  return {
    name: 'elk',
    nodeDimensionsIncludeLabels: true,
    fit: false,
    padding: 60,
    animate: true,
    animationDuration: 250,

    // Core ELK options - passed through to elk.js verbatim
    elk: {
      algorithm: 'layered',

      // Left → right attack chain
      'elk.direction': 'RIGHT',

      // Inter-layer (horizontal) gap between attack steps
      'elk.layered.spacing.nodeNodeBetweenLayers': '130',

      // Gap between nodes within the same layer
      'elk.spacing.nodeNode': '40',

      // Organic-looking edge curves
      'elk.edgeRouting': 'SPLINES',

      // Respect elk.position hints so existing nodes don't jump around
      'elk.interactiveLayout': 'true',

      // Stability: prefer keeping the relative order of nodes across layout runs
      'elk.layered.considerModelOrder.strategy': 'NODES_AND_EDGES',

      // Reduce crossings
      'elk.layered.crossingMinimization.strategy': 'LAYER_SWEEP',

      // Keep compound node children tightly packed
      'elk.padding': '[top=20,left=20,bottom=20,right=20]',
    },

    // Per-node ELK options
    nodeLayoutOptions: (node: cytoscape.NodeSingular) => {
      const opts: Record<string, string> = {};

      // Fix existing nodes in place via interactive layout hint
      const pos = positions[node.id()];
      if (pos) {
        opts['elk.position'] = elkPos(pos.x, pos.y);
      }

      if (node.isParent()) {
        // Namespace compounds: local force-directed layout keeps children close
        opts['elk.algorithm'] = 'stress';
        opts['elk.stress.desiredEdgeLength'] = '55';
        // Looser padding inside compounds to avoid crowding
        opts['elk.padding'] = '[top=15,left=15,bottom=15,right=15]';
      } else {
        // Leaf nodes: hint the preferred layer based on kind
        const kind: string = node.data('kind') ?? '';
        const layer = NODE_LAYER[kind];
        if (layer !== undefined) {
          opts['elk.layered.layering.layer'] = String(layer);
        }
      }

      return opts;
    },

    // Per-edge: exclude informational edges from the layout graph
    // (they are still displayed by Cytoscape but don't influence positions)
    edgeLayoutOptions: (edge: cytoscape.EdgeSingular) => {
      if (!isLayoutEdge(edge)) {
        // Returning undefined causes cytoscape-elk to skip this edge in the ELK model
        return undefined;
      }
      return {};
    },
  };
}
```

- [ ] **Step 2: Verify TypeScript compiles**

```bash
cd frontend
pnpm exec svelte-check --tsconfig tsconfig.json 2>&1 | grep -E "elk_layout|error" | head -20
```

Expected: no errors mentioning `elk_layout.ts`. (Other pre-existing errors are acceptable.)

- [ ] **Step 3: Commit**

```bash
git add frontend/src/routes/components/elk_layout.ts
git commit -m "feat: add ELK layout options builder with layer assignments"
```

---

## Task 3: Prune `graph_style.ts`

**Files:**
- Modify: `frontend/src/routes/components/graph_style.ts`

Remove the `createLayout` function and `layout` constant - they are replaced by `elk_layout.ts`. Keep everything else (styles, `applyCompromisedStyle`, `isValidPosition`).

- [ ] **Step 1: Remove the layout exports**

Delete lines 22–165 (the `isValidPosition` helper, `createLayout` function, and `layout` constant) from `graph_style.ts`. The file should now start at the `kind_svg_map` constant.

The `isValidPosition` function is still useful - move it to `elk_layout.ts` and re-export, or simply inline the validation in `graph.svelte` where positions are loaded. The simplest approach: move it to `elk_layout.ts` as an exported helper.

Add to the **top** of `elk_layout.ts` (before the `NODE_LAYER` const):

```typescript
/** Rejects positions with non-finite coordinates or extreme values. */
export function isValidPosition(pos: unknown): pos is { x: number; y: number } {
  if (!pos || typeof pos !== 'object') return false;
  const { x, y } = pos as { x: number; y: number };
  return (
    typeof x === 'number' &&
    typeof y === 'number' &&
    isFinite(x) &&
    isFinite(y) &&
    !isNaN(x) &&
    !isNaN(y) &&
    Math.abs(x) < 1e6 &&
    Math.abs(y) < 1e6
  );
}
```

- [ ] **Step 2: Verify TypeScript compiles**

```bash
cd frontend
pnpm exec svelte-check --tsconfig tsconfig.json 2>&1 | grep -E "graph_style|error" | head -20
```

Expected: no new errors in `graph_style.ts`.

- [ ] **Step 3: Commit**

```bash
git add frontend/src/routes/components/graph_style.ts \
        frontend/src/routes/components/elk_layout.ts
git commit -m "refactor: move layout logic out of graph_style into elk_layout"
```

---

## Task 4: Wire ELK into `graph.svelte`

**Files:**
- Modify: `frontend/src/routes/components/graph.svelte`

Three changes: (a) swap the import/registration, (b) replace `createLayout` call, (c) remove the fcose-specific `existingNodes.lock()`/`.unlock()` pattern since ELK uses position hints instead.

- [ ] **Step 1: Swap the import block**

Replace:
```typescript
import cytoscape from 'cytoscape';
import fcose from 'cytoscape-fcose';
// @ts-ignore
import expandCollapse from 'cytoscape-expand-collapse';
import { toaster } from '$lib/components/toaster';

import { getGraphStyle, layout, createLayout, applyCompromisedStyle } from './graph_style';
```

With:
```typescript
import cytoscape from 'cytoscape';
// @ts-ignore
import elk from 'cytoscape-elk';
// @ts-ignore
import expandCollapse from 'cytoscape-expand-collapse';
import { toaster } from '$lib/components/toaster';

import { getGraphStyle, applyCompromisedStyle } from './graph_style';
import { createElkLayout, isValidPosition } from './elk_layout';
```

- [ ] **Step 2: Swap the plugin registration**

Replace:
```typescript
cytoscape.use(fcose);
```

With:
```typescript
cytoscape.use(elk);
```

- [ ] **Step 3: Remove the `existingNodes` tracking variables**

Delete these two lines near the top of the `<script>` block:
```typescript
let existingNodes: cytoscape.NodeCollection = cytoscape().collection();
```
(The `previousNodeIds` variable stays - it's still used for the new-node detection logic.)

- [ ] **Step 4: Update the initial `cy` construction**

The `layout` prop passed to `cytoscape({...})` on `onMount` used the old `layout` constant. Replace it with an empty layout (we run layout explicitly in the `$effect`):

Replace:
```typescript
cy = cytoscape({
  container: graphContainer,
  elements: { nodes: nodes, edges: edges },
  style: getGraphStyle(theme.isDark),
  layout: layout,
  zoom: zoom,
  wheelSensitivity: 0.1
});
```

With:
```typescript
cy = cytoscape({
  container: graphContainer,
  elements: { nodes: nodes, edges: edges },
  style: getGraphStyle(theme.isDark),
  layout: { name: 'preset' },
  zoom: zoom,
  wheelSensitivity: 0.1
});
```

- [ ] **Step 5: Replace the layout invocation inside the `$effect`**

Find the block that currently reads (around line 370–500):

```typescript
if (hasNewNodes || hasFewerNodes || previousNodeIds.size === 0) {
  // ...
  existingNodes = cy.nodes().filter(...);
  existingNodes.lock();
  // ...
  const enhancedLayout = createLayout(visibleNodes, positions);
  // ...
  enhancedLayout.stop = () => {
    existingNodes.unlock();
    // ...
  };
  cy.layout(enhancedLayout).run();
```

Replace the entire `if (hasNewNodes || hasFewerNodes || previousNodeIds.size === 0)` block with:

```typescript
if (hasNewNodes || hasFewerNodes || previousNodeIds.size === 0) {
  console.log(`Graph changed: ${hasNewNodes ? 'new nodes' : hasFewerNodes ? 'nodes removed' : 'initial load'}`);

  const containerRect = graphContainer.getBoundingClientRect();
  if (containerRect.width === 0 || containerRect.height === 0) {
    console.warn('Graph container has zero dimensions, skipping layout');
    previousNodeIds = currentNodeIds;
    return;
  }

  const currentPan = cy.pan();
  const currentZoom = cy.zoom();
  const isInitialLoad = previousNodeIds.size === 0;

  // Build layout options - positions are passed as elk.position hints so
  // existing nodes are treated as "preferred" locations by elk.interactiveLayout
  const layoutOptions = createElkLayout(positions);

  // Run layout on visible non-informational elements only.
  // All nodes are included so nothing goes missing; informational edges
  // are excluded via edgeLayoutOptions returning undefined in elk_layout.ts.
  const elements = cy.elements(':visible');

  const l = elements.layout(layoutOptions as any);

  l.one('layoutstop', () => {
    if (isInitialLoad) {
      cy.fit(undefined, 50);
      if (cy.zoom() > 2) cy.zoom(2);
      cy.center();
    } else {
      if (currentPan && (currentPan.x !== 0 || currentPan.y !== 0)) {
        cy.pan(currentPan);
      }
      cy.zoom(currentZoom);
    }
    savePositions();
    console.log('ELK layout complete');
  });

  l.run();
  previousNodeIds = currentNodeIds;
}
```

- [ ] **Step 6: Update `loadPositions` to use the imported `isValidPosition`**

The `loadPositions` function currently has an inline validation. Replace the inline check with the imported helper:

```typescript
function loadPositions(): PosMap {
  if (!browser) return {};
  try {
    const stored = JSON.parse(sessionStorage.getItem(POS_KEY) ?? '{}');
    const validated: PosMap = {};
    for (const [id, pos] of Object.entries(stored)) {
      if (isValidPosition(pos)) {
        validated[id] = pos as Pos;
      } else {
        console.warn(`Invalid position for node ${id}, skipping`);
      }
    }
    return validated;
  } catch (e) {
    console.error('Error loading positions:', e);
    return {};
  }
}
```

- [ ] **Step 7: Verify TypeScript compiles**

```bash
cd frontend
pnpm exec svelte-check --tsconfig tsconfig.json 2>&1 | grep -iE "error|elk" | head -30
```

Expected: no type errors in `graph.svelte` related to elk or the removed imports.

- [ ] **Step 8: Commit**

```bash
git add frontend/src/routes/components/graph.svelte
git commit -m "feat: migrate graph layout from fcose to ELK layered+stress"
```

---

## Task 5: Run and tune layout parameters

The ELK parameters in `elk_layout.ts` are starting points. After seeing the layout in a browser, expect to tweak spacing values. This task is about validating the layout looks right and adjusting the knobs.

**Files:**
- Modify: `frontend/src/routes/components/elk_layout.ts` (spacing values only)

- [ ] **Step 1: Start the dev server**

```bash
cd frontend
pnpm dev
```

Open the app and load a campaign with a Kubernetes graph. Verify:
- Ran/C2 node is on the left
- Attack flow reads left to right
- Namespace compounds group their children tightly
- No node overlap within compounds
- Edges route with curves (SPLINES), not harsh angles

- [ ] **Step 2: Tune inter-layer spacing if nodes feel too far apart or too cramped**

In `elk_layout.ts`, adjust:

```typescript
'elk.layered.spacing.nodeNodeBetweenLayers': '130',  // increase for more breathing room
'elk.spacing.nodeNode': '40',                         // within-layer vertical gap
```

Typical useful range: `80–200` for between-layers, `20–60` for node-node.

- [ ] **Step 3: Tune compound stress layout if namespace children overlap**

In `elk_layout.ts` `nodeLayoutOptions`, adjust:

```typescript
opts['elk.stress.desiredEdgeLength'] = '55';  // lower = nodes pack tighter
opts['elk.padding'] = '[top=15,left=15,bottom=15,right=15]';
```

- [ ] **Step 4: Verify incremental update stability**

In the running app, trigger an action that adds a node to the graph (execute a TTP). Confirm:
- Existing nodes do not jump to new positions
- New node appears near its connected neighbor (pre-positioned in graph.svelte)
- Layout runs and settles without existing nodes moving significantly

If existing nodes do move significantly: lower `elk.stress.desiredEdgeLength` or increase the weight of the `elk.position` hint by also setting `elk.interactiveLayout: 'true'` on the per-node `nodeLayoutOptions` for nodes that have a saved position (as a belt-and-suspenders approach).

- [ ] **Step 5: Verify namespace hide/show filter still works**

Toggle namespace visibility in the filter panel. Confirm the graph re-layouts correctly after hiding and re-showing a namespace.

- [ ] **Step 6: Verify collapse/expand still works**

Collapse a namespace compound via the expand-collapse cue (top-left corner of the compound). Confirm the collapsed node shows as a single node, and expanding restores children at their correct positions.

- [ ] **Step 7: Commit tuned values**

```bash
git add frontend/src/routes/components/elk_layout.ts
git commit -m "chore: tune ELK spacing parameters after visual validation"
```

---

## Task 6: Remove `cytoscape-fcose` dependency

Once the layout is confirmed working and stable, clean up the unused dependency.

**Files:**
- Modify: `frontend/package.json`

- [ ] **Step 1: Remove fcose**

```bash
cd frontend
pnpm remove cytoscape-fcose
```

- [ ] **Step 2: Verify no remaining imports**

```bash
grep -r "fcose\|createLayout\|from.*graph_style.*layout" frontend/src --include="*.ts" --include="*.svelte"
```

Expected: no matches.

- [ ] **Step 3: Verify dev server still starts cleanly**

```bash
pnpm dev
```

Expected: no import errors.

- [ ] **Step 4: Commit**

```bash
git add frontend/package.json frontend/pnpm-lock.yaml
git commit -m "chore: remove unused cytoscape-fcose dependency"
```

---

## Self-Review

**Spec coverage:**

| Requirement | Task that covers it |
|---|---|
| Consistent layout on navigation / back-forward | Task 4 step 5: `elk.interactiveLayout` + position hints |
| Compound nodes with minimal overlap | Task 2: `stress` sub-algorithm within namespace compounds |
| Only certain edges drive layout | Task 2: `edgeLayoutOptions` returns `undefined` for informational edges |
| Progressively evolving nodes | Task 4 step 5: pre-positioning near neighbors kept from existing code, layout only re-runs on structural change |
| Ran/C2 on the left, flow left-to-right | Task 2: `elk.direction: RIGHT` + `NODE_LAYER[C2] = 0` |
| Namespace children close together | Task 2: `stress` with `desiredEdgeLength: 55` + tight padding |
| ServiceAccount above Pod | Partially covered by `NODE_LAYER` (SA=5, Pod=4) - within a namespace compound this is best-effort from the stress layout |
| k8s Nodes southward | `NODE_LAYER[Node] = 7` places them in a late layer; within their layer, ELK crossing minimisation handles y ordering |
| Layer visibility (show/hide relations) | Existing `hideRedundantInformationalEdges` + namespace filter logic unchanged; this plan lays groundwork for a future edge-type visibility toggle |

**Notes on partial requirements:**
- *ServiceAccount above Pod within a compound*: `stress` doesn't enforce directional ordering. If this matters, a future iteration can switch compound sub-algorithm to `layered` with `direction: UP` for Namespace nodes specifically - but that requires the parent-child edges to have direction, which may not be the case.
- *Edge layer visibility*: the plan keeps the existing informational edge show/hide logic. A dedicated relation-type visibility panel is out of scope for this migration and should be a separate task.

**No placeholder scan:** All steps include exact code or exact commands. No "TBD" or "add appropriate error handling" patterns.

**Type consistency check:** `createElkLayout` is defined in Task 2 and called in Task 4. `isValidPosition` is added to `elk_layout.ts` in Task 3 and imported in Task 4 (step 6). `NODE_LAYER` is defined in Task 2 and used internally in Task 2. No mismatches.
