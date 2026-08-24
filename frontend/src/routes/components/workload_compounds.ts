import type { Node } from '$lib/api/index';

export const WORKLOAD_KINDS = new Set([
	'Deployment',
	'ReplicaSet',
	'StatefulSet',
	'DaemonSet',
	'Job'
]);

/** Workloads with discovered pods should initially render as collapsed compounds. */
export function workloadCompoundIds(nodes: Node[]): Set<string> {
	const podCounts = new Map<string, number>();

	for (const node of nodes) {
		if (node.kind !== 'Pod' || !node.parent) continue;
		podCounts.set(node.parent, (podCounts.get(node.parent) ?? 0) + 1);
	}

	return new Set(
		nodes
			.filter((node) => WORKLOAD_KINDS.has(node.kind) && (podCounts.get(node.id) ?? 0) > 0)
			.map((node) => node.id)
	);
}
