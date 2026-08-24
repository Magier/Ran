import { describe, expect, it } from 'vitest';
import type { Node } from '$lib/api/index';
import { workloadCompoundIds } from './workload_compounds';

function node(id: string, kind: string, parent?: string): Node {
	return { id, entityId: id, kind, name: id, parent };
}

describe('workloadCompoundIds', () => {
	it('selects a workload that owns one pod', () => {
		const nodes = [
			node('deployment/one', 'Deployment'),
			node('pod/one', 'Pod', 'deployment/one')
		];

		expect([...workloadCompoundIds(nodes)]).toEqual(['deployment/one']);
	});

	it('selects workload compounds with multiple pod children', () => {
		const nodes = [
			node('deployment/many', 'Deployment'),
			node('pod/one', 'Pod', 'deployment/many'),
			node('pod/two', 'Pod', 'deployment/many')
		];

		expect([...workloadCompoundIds(nodes)]).toEqual(['deployment/many']);
	});

	it('does not treat non-workload compounds as workloads', () => {
		const nodes = [
			node('ns/default', 'Namespace'),
			node('pod/one', 'Pod', 'ns/default'),
			node('pod/two', 'Pod', 'ns/default')
		];

		expect([...workloadCompoundIds(nodes)]).toEqual([]);
	});
});
