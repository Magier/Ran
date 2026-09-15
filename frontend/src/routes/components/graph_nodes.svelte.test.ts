import { describe, expect, it } from 'vitest';
import cytoscape from 'cytoscape';
import {
	toCyNode,
	syncNodeParent,
	preserveSurvivingDescendants,
	ancestorsToRevealAfterReparent
} from './graph_nodes';
import type { PosMap } from './graph_nodes';
import type { Node } from '$lib/api/index';

function node(overrides: Partial<Node> & Pick<Node, 'id'>): Node {
	return {
		name: overrides.id,
		kind: 'Pod',
		entityId: overrides.id,
		...overrides
	} as Node;
}

function mountCy(nodes: Node[], positions: PosMap = {}) {
	return cytoscape({
		headless: true,
		styleEnabled: true,
		elements: nodes.map((n) => toCyNode(n, positions))
	});
}

/** Mirrors the refresh loop in graph.svelte: merge data, then reconcile parents. */
function refresh(cy: cytoscape.Core, nodes: Node[]) {
	const positions: PosMap = {};
	nodes
		.map((n) => toCyNode(n, positions))
		.forEach((n) => {
			cy.getElementById(n.data.id).data(n.data);
			syncNodeParent(cy, n);
		});
}

function parentOf(cy: cytoscape.Core, id: string): string | null {
	const el = cy.getElementById(id);
	return el.isChild() ? el.parent().first().id() : null;
}

describe('toCyNode', () => {
	it('carries a parent through unguarded', () => {
		expect(toCyNode(node({ id: 'pod-a', parent: 'ns/x' }), {}).data.parent).toBe('ns/x');
	});

	it('leaves the node at the root when the backend sends no parent', () => {
		expect(toCyNode(node({ id: 'pod-a' }), {}).data.parent).toBeUndefined();
	});

	it('leaves compromised and isRunning absent when the backend omits them', () => {
		// The entity panel reads a missing key as "not applicable"; neither field
		// can latch because the backend only omits it for kinds that never use it.
		const data = toCyNode(node({ id: 'svc-a', kind: 'Service' }), {}).data;

		expect(Object.hasOwn(data, 'compromised')).toBe(false);
		expect(Object.hasOwn(data, 'isRunning')).toBe(false);
	});

	it('keeps an explicit false rather than dropping it', () => {
		const data = toCyNode(node({ id: 'pod-a', isRunning: false, compromised: false }), {}).data;

		expect(data.isRunning).toBe(false);
		expect(data.compromised).toBe(false);
	});

	it('derives scenarioProvided from provenance', () => {
		expect(toCyNode(node({ id: 'a', provenance: ['scenario'] }), {}).data.scenarioProvided).toBe(
			true
		);
		expect(toCyNode(node({ id: 'b', provenance: ['action'] }), {}).data.scenarioProvided).toBe(
			false
		);
	});

	it('seeds the operator node position and records it in the position map', () => {
		const positions: PosMap = {};

		expect(toCyNode(node({ id: 'c2/Ran', name: 'Ran' }), positions).position).toEqual({
			x: -100,
			y: 0
		});
		expect(positions['c2/Ran']).toEqual({ x: -100, y: 0 });
	});

	it('prefers a saved position over the default seed', () => {
		const positions: PosMap = { 'c2/Ran': { x: 5, y: 6 } };

		expect(toCyNode(node({ id: 'c2/Ran', name: 'Ran' }), positions).position).toEqual({
			x: 5,
			y: 6
		});
	});
});

describe('syncNodeParent', () => {
	it('is needed because cytoscape drops a parent handed to data()', () => {
		// The load-bearing fact behind this helper. If a cytoscape bump ever makes
		// data() honour `parent`, this test fails and the helper can be revisited.
		const cy = mountCy([
			node({ id: 'ns/x', kind: 'Namespace' }),
			node({ id: 'ns/y', kind: 'Namespace' }),
			node({ id: 'pod-a', parent: 'ns/x' })
		]);

		cy.getElementById('pod-a').data(toCyNode(node({ id: 'pod-a', parent: 'ns/y' }), {}).data);

		expect(parentOf(cy, 'pod-a')).toBe('ns/x');
		expect(cy.getElementById('pod-a').data('parent')).toBe('ns/x');
	});

	it('detaches a node that lost its parent', () => {
		// The #84 hazard: the refresh updates existing nodes with data(), which
		// cytoscape ignores for `parent`, so without an explicit move the node
		// stays rendered inside a compound it no longer belongs to.
		const cy = mountCy([
			node({ id: 'ns/x', kind: 'Namespace' }),
			node({ id: 'pod-a', parent: 'ns/x' })
		]);
		expect(parentOf(cy, 'pod-a')).toBe('ns/x');

		refresh(cy, [node({ id: 'ns/x', kind: 'Namespace' }), node({ id: 'pod-a' })]);

		expect(parentOf(cy, 'pod-a')).toBeNull();
	});

	it('moves a node whose parent changed', () => {
		const cy = mountCy([
			node({ id: 'ns/x', kind: 'Namespace' }),
			node({ id: 'ns/y', kind: 'Namespace' }),
			node({ id: 'pod-a', parent: 'ns/x' })
		]);

		refresh(cy, [
			node({ id: 'ns/x', kind: 'Namespace' }),
			node({ id: 'ns/y', kind: 'Namespace' }),
			node({ id: 'pod-a', parent: 'ns/y' })
		]);

		expect(parentOf(cy, 'pod-a')).toBe('ns/y');
	});

	it('is a no-op when the parent is unchanged', () => {
		const cy = mountCy([
			node({ id: 'ns/x', kind: 'Namespace' }),
			node({ id: 'pod-a', parent: 'ns/x' })
		]);

		expect(syncNodeParent(cy, toCyNode(node({ id: 'pod-a', parent: 'ns/x' }), {}))).toBe(false);
		expect(parentOf(cy, 'pod-a')).toBe('ns/x');
	});

	it('keeps connected edges and classes across a move', () => {
		const cy = mountCy([
			node({ id: 'ns/x', kind: 'Namespace' }),
			node({ id: 'ns/y', kind: 'Namespace' }),
			node({ id: 'pod-a', parent: 'ns/x' }),
			node({ id: 'node-1', kind: 'Node' })
		]);
		cy.add({
			group: 'edges',
			data: { id: 'e1', source: 'pod-a', target: 'node-1', name: 'runs-on' }
		});
		cy.getElementById('pod-a').addClass('highlighted');

		syncNodeParent(cy, toCyNode(node({ id: 'pod-a', parent: 'ns/y' }), {}));

		expect(parentOf(cy, 'pod-a')).toBe('ns/y');
		expect(cy.getElementById('pod-a').hasClass('highlighted')).toBe(true);
		expect(cy.getElementById('e1').data('name')).toBe('runs-on');
	});

	it('leaves the node alone when the new parent is not in the graph yet', () => {
		const cy = mountCy([
			node({ id: 'ns/x', kind: 'Namespace' }),
			node({ id: 'pod-a', parent: 'ns/x' })
		]);

		expect(syncNodeParent(cy, toCyNode(node({ id: 'pod-a', parent: 'ns/gone' }), {}))).toBe(false);
		expect(parentOf(cy, 'pod-a')).toBe('ns/x');
	});

	it('ignores a node that is not in the graph', () => {
		const cy = mountCy([node({ id: 'pod-a' })]);

		expect(syncNodeParent(cy, toCyNode(node({ id: 'ghost' }), {}))).toBe(false);
	});

	it('is a no-op for a root node that still has no parent', () => {
		const cy = mountCy([node({ id: 'pod-a' })]);

		expect(syncNodeParent(cy, toCyNode(node({ id: 'pod-a' }), {}))).toBe(false);
	});

	it('attaches a node that gained a parent', () => {
		const cy = mountCy([node({ id: 'ns/x', kind: 'Namespace' }), node({ id: 'pod-a' })]);

		refresh(cy, [node({ id: 'ns/x', kind: 'Namespace' }), node({ id: 'pod-a', parent: 'ns/x' })]);

		expect(parentOf(cy, 'pod-a')).toBe('ns/x');
	});
});

describe('ancestorsToRevealAfterReparent', () => {
	it('reveals every collapsed ancestor of a reparented compromised pod', () => {
		const cy = mountCy([
			node({ id: 'cluster', kind: 'K8sCluster' }),
			node({ id: 'ns/x', kind: 'Namespace', parent: 'cluster' }),
			node({ id: 'pod-a' })
		]);
		const updated = toCyNode(node({ id: 'pod-a', parent: 'ns/x', compromised: true }), {});
		const moved = syncNodeParent(cy, updated);

		expect(ancestorsToRevealAfterReparent(cy, updated, moved)).toEqual(['ns/x', 'cluster']);
	});

	it('does not alter collapse state for an uncompromised or unmoved node', () => {
		const cy = mountCy([
			node({ id: 'ns/x', kind: 'Namespace' }),
			node({ id: 'pod-a', parent: 'ns/x' })
		]);
		const uncompromised = toCyNode(node({ id: 'pod-a', parent: 'ns/x' }), {});
		const compromised = toCyNode(node({ id: 'pod-a', parent: 'ns/x', compromised: true }), {});

		expect(ancestorsToRevealAfterReparent(cy, uncompromised, true)).toEqual([]);
		expect(ancestorsToRevealAfterReparent(cy, compromised, false)).toEqual([]);
	});
});

describe('preserveSurvivingDescendants', () => {
	it('keeps a pod alive when its obsolete compound parent is removed', () => {
		const cy = mountCy([
			node({ id: 'cluster-old', kind: 'K8sCluster' }),
			node({ id: 'pod-a', parent: 'cluster-old', compromised: true })
		]);

		expect(preserveSurvivingDescendants(cy, new Set(['cluster-new', 'pod-a']))).toEqual(['pod-a']);
		cy.getElementById('cluster-old').remove();

		expect(cy.getElementById('pod-a').nonempty()).toBe(true);
		expect(parentOf(cy, 'pod-a')).toBeNull();
	});

	it('allows the surviving pod to be attached to the replacement cluster', () => {
		const cy = mountCy([
			node({ id: 'cluster-old', kind: 'K8sCluster' }),
			node({ id: 'pod-a', parent: 'cluster-old', compromised: true })
		]);
		preserveSurvivingDescendants(cy, new Set(['cluster-new', 'pod-a']));
		cy.getElementById('cluster-old').remove();
		cy.add(toCyNode(node({ id: 'cluster-new', kind: 'K8sCluster' }), {}));

		expect(
			syncNodeParent(
				cy,
				toCyNode(node({ id: 'pod-a', parent: 'cluster-new', compromised: true }), {})
			)
		).toBe(true);
		expect(parentOf(cy, 'pod-a')).toBe('cluster-new');
	});
});
