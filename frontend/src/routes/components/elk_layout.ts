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

/** Serialise a position for the elk.position layout option. */
function elkPos(x: number, y: number): string {
  return `(${Math.round(x)},${Math.round(y)})`;
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

    elk: {
      'elk.algorithm': 'layered',
      'elk.direction': 'RIGHT',
      'elk.layered.spacing.nodeNodeBetweenLayers': '130',
      'elk.spacing.nodeNode': '40',
      'elk.edgeRouting': 'SPLINES',
      'elk.interactiveLayout': 'true',
      'elk.layered.considerModelOrder.strategy': 'NODES_AND_EDGES',
      'elk.layered.crossingMinimization.strategy': 'LAYER_SWEEP',
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
        opts['elk.stress.desiredEdgeLength'] = '55';
        opts['elk.padding'] = '[top=15,left=15,bottom=15,right=15]';
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
      if (INFORMATIONAL_EDGES.has(edge.data('name'))) {
        return undefined;
      }
      return {};
    },
  };
}
