import type cytoscape from 'cytoscape';
import { INFORMATIONAL_EDGES } from './edge_categories';

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

export type LayeringStrategy = 'NETWORK_SIMPLEX' | 'LONGEST_PATH' | 'INTERACTIVE' | 'MIN_WIDTH';
export type NodePlacementStrategy = 'BRANDES_KOEPF' | 'NETWORK_SIMPLEX' | 'LINEAR_SEGMENTS' | 'SIMPLE';

export type LayoutParams = {
  // Spacing
  layerSpacing: number;       // horizontal gap between attack-chain layers
  nodeSpacing: number;        // vertical gap between nodes in the same layer
  edgeNodeSpacing: number;    // gap between edges and nodes across layer boundaries
  aspectRatio: number;        // target width/height ratio for the overall layout
  // Compound (namespace) sub-layout
  compoundEdgeLength: number; // stress target edge length within namespace compounds
  compoundPadding: number;    // padding inside namespace compound nodes
  stressIterations: number;   // max iterations of stress algorithm inside compounds
  // Edge behaviour
  usesStraightness: number;   // 0–10: how hard ELK tries to align "uses" edge endpoints vertically
  // Animation
  animationDuration: number;  // ms; 0 = instant
  // Strategies
  layeringStrategy: LayeringStrategy;
  nodePlacementStrategy: NodePlacementStrategy;
};

export const DEFAULT_LAYOUT_PARAMS: LayoutParams = {
  layerSpacing: 130,
  nodeSpacing: 40,
  edgeNodeSpacing: 20,
  aspectRatio: 1.6,
  compoundEdgeLength: 55,
  compoundPadding: 15,
  stressIterations: 300,
  usesStraightness: 3,
  animationDuration: 250,
  layeringStrategy: 'NETWORK_SIMPLEX',
  nodePlacementStrategy: 'BRANDES_KOEPF',
};

/** Serialise a position for the elk.position layout option. */
function elkPos(x: number, y: number): string {
  return `(${Math.round(x)},${Math.round(y)})`;
}

/**
 * Builds a Cytoscape layout options object that drives ELK.
 *
 * @param positions - Saved positions from sessionStorage; used as hints for
 *                    elk.interactiveLayout so existing nodes don't move.
 * @param params - Tunable spacing parameters (defaults to DEFAULT_LAYOUT_PARAMS).
 */
export function createElkLayout(
  positions: Record<string, { x: number; y: number }> = {},
  params: LayoutParams = DEFAULT_LAYOUT_PARAMS
): cytoscape.LayoutOptions & Record<string, unknown> {
  const p = params.compoundPadding;
  const compoundPad = `[top=${p},left=${p},bottom=${p},right=${p}]`;

  return {
    name: 'elk',
    nodeDimensionsIncludeLabels: true,
    fit: false,
    padding: 60,
    animate: params.animationDuration > 0,
    animationDuration: params.animationDuration,

    elk: {
      'elk.algorithm': 'layered',
      'elk.direction': 'RIGHT',
      'elk.aspectRatio': String(params.aspectRatio),
      'elk.layered.spacing.nodeNodeBetweenLayers': String(params.layerSpacing),
      'elk.spacing.nodeNode': String(params.nodeSpacing),
      'elk.layered.spacing.edgeNodeBetweenLayers': String(params.edgeNodeSpacing),
      'elk.edgeRouting': 'SPLINES',
      'elk.interactiveLayout': 'true',
      'elk.layered.considerModelOrder.strategy': 'NODES_AND_EDGES',
      'elk.layered.crossingMinimization.strategy': 'LAYER_SWEEP',
      'elk.layered.layering.strategy': params.layeringStrategy,
      'elk.layered.nodePlacement.strategy': params.nodePlacementStrategy,
      'elk.padding': '[top=20,left=20,bottom=20,right=20]',
    },

    nodeLayoutOptions: (node: cytoscape.NodeSingular) => {
      const opts: Record<string, string> = {};

      const pos = positions[node.id()];
      if (pos) {
        opts['elk.position'] = elkPos(pos.x, pos.y);
      }

      if (node.isParent()) {
        opts['elk.algorithm'] = 'stress';
        opts['elk.stress.desiredEdgeLength'] = String(params.compoundEdgeLength);
        opts['elk.stress.iterations'] = String(params.stressIterations);
        opts['elk.padding'] = compoundPad;
      } else {
        const kind: string = node.data('kind') ?? '';
        const layer = NODE_LAYER[kind];
        if (layer !== undefined) {
          opts['elk.layered.layering.layer'] = String(layer);
        }
      }

      return opts;
    },

    edgeLayoutOptions: (edge: cytoscape.EdgeSingular) => {
      const name: string = edge.data('name');
      if (name === 'uses') {
        return { 'elk.layered.priority.straightness': String(params.usesStraightness) };
      }
      if (INFORMATIONAL_EDGES.has(name)) {
        return undefined;
      }
      return {};
    },
  };
}
