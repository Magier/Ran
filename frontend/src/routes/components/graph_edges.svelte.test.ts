import { describe, expect, it } from 'vitest';
import cytoscape from 'cytoscape';
import {
	consolidateCollapsedEdges,
	restoreConsolidatedEdges,
	reconcileCollapsedEdges,
	hideRedundantInformationalEdges,
	COLLAPSED_EDGE_CLASS
} from './graph_edges';

const COLLAPSED_NODE_CLASS = 'cy-expand-collapse-collapsed-node';

// These tests exercise the edge-visibility helpers directly against a headless
// cytoscape instance. We deliberately do NOT drive the real cytoscape-expand-collapse
// plugin: it needs a rendered canvas, and its async renderer teardown throws
// unhandled errors under jsdom (which Vitest treats as failures). Instead we
// simulate the post-collapse graph state the plugin produces - every child edge
// re-pointed at the collapsed compound, and the compound carrying the collapsed
// class - which is exactly the input our helpers consume.

function mountCy(elements: any[]) {
	return cytoscape({ headless: true, styleEnabled: true, elements });
}

/** Mark a compound as collapsed, mirroring the plugin's class. */
function markCollapsed(cy: any, id: string) {
	cy.getElementById(id).addClass(COLLAPSED_NODE_CLASS);
}

function visibleEdges(cy: any): string[] {
	return cy
		.edges()
		.filter((e: any) => e.visible())
		.map((e: any) => `${e.source().id()}->${e.target().id()} [${e.id()}]`)
		.sort();
}

/**
 * Post-collapse state of a namespace whose pods all `runs-on` the same external
 * node: the plugin has already re-pointed each edge's source to the namespace.
 */
function collapsedNsRunsOn() {
	return [
		{ data: { id: 'ns' } },
		{ data: { id: 'nodeX' } },
		{ data: { id: 'e1', source: 'ns', target: 'nodeX', name: 'runs-on', informational: true } },
		{ data: { id: 'e2', source: 'ns', target: 'nodeX', name: 'runs-on', informational: true } },
		{ data: { id: 'e3', source: 'ns', target: 'nodeX', name: 'runs-on', informational: true } }
	];
}

describe('consolidateCollapsedEdges', () => {
	it('collapses parallel same-pair edges into one meta-edge and hides the children', () => {
		const cy = mountCy(collapsedNsRunsOn());
		markCollapsed(cy, 'ns');

		consolidateCollapsedEdges(cy, cy.getElementById('ns'));

		expect(visibleEdges(cy)).toEqual(['ns->nodeX [meta-ns-to-nodeX]']);
	});

	it('keeps the collapsed edges hidden when informational filtering re-runs', () => {
		const cy = mountCy(collapsedNsRunsOn());
		markCollapsed(cy, 'ns');

		consolidateCollapsedEdges(cy, cy.getElementById('ns'));
		// The reactive effect re-applies informational filtering after every update.
		hideRedundantInformationalEdges(cy);

		// Regression: previously this re-showed all three per-pod runs-on children.
		expect(visibleEdges(cy)).toEqual(['ns->nodeX [meta-ns-to-nodeX]']);
	});

	it('tags consolidated children with the collapse marker class', () => {
		const cy = mountCy(collapsedNsRunsOn());
		markCollapsed(cy, 'ns');

		consolidateCollapsedEdges(cy, cy.getElementById('ns'));

		['e1', 'e2', 'e3'].forEach((id) => {
			expect(cy.getElementById(id).hasClass(COLLAPSED_EDGE_CLASS)).toBe(true);
		});
	});

	it('inherits informational styling when all consolidated edges are informational', () => {
		const cy = mountCy(collapsedNsRunsOn());
		markCollapsed(cy, 'ns');

		consolidateCollapsedEdges(cy, cy.getElementById('ns'));
		hideRedundantInformationalEdges(cy);

		const meta = cy.getElementById('meta-ns-to-nodeX');
		expect(meta.length).toBe(1);
		expect(meta.data('informational')).toBe(true);
		expect(meta.visible()).toBe(true);
	});

	it('does NOT mark the meta-edge informational when any child is actionable', () => {
		const cy = mountCy([
			{ data: { id: 'ns' } },
			{ data: { id: 'nodeX' } },
			// runs-on is informational, exploits is actionable - same directed pair
			{ data: { id: 'e1', source: 'ns', target: 'nodeX', name: 'runs-on', informational: true } },
			{ data: { id: 'e2', source: 'ns', target: 'nodeX', name: 'exploits' } }
		]);
		markCollapsed(cy, 'ns');

		consolidateCollapsedEdges(cy, cy.getElementById('ns'));

		const meta = cy.getElementById('meta-ns-to-nodeX');
		expect(meta.length).toBe(1);
		expect(meta.data('informational')).toBe(false);
	});

	it('restores the original edges and clears the marker on expand', () => {
		const cy = mountCy(collapsedNsRunsOn());
		markCollapsed(cy, 'ns');
		consolidateCollapsedEdges(cy, cy.getElementById('ns'));

		restoreConsolidatedEdges(cy, cy.getElementById('ns'));

		// Meta-edge gone; original edges restored and the collapse marker cleared.
		expect(cy.getElementById('meta-ns-to-nodeX').length).toBe(0);
		['e1', 'e2', 'e3'].forEach((id) => {
			const edge = cy.getElementById(id);
			expect(edge.hasClass(COLLAPSED_EDGE_CLASS)).toBe(false);
			expect(edge.visible()).toBe(true);
		});
	});

	it('merges existing child meta-edges upward (nested collapsed compounds)', () => {
		// Namespace with two already-collapsed deployments, each previously
		// consolidated into its own meta-edge, both re-pointed at the namespace
		// after it collapses. Consolidating the namespace must merge them into one.
		const cy = mountCy([
			{ data: { id: 'ns' } },
			{ data: { id: 'nodeX' } },
			{
				data: {
					id: 'meta-deployA-to-nodeX',
					source: 'ns',
					target: 'nodeX',
					name: 'runs-on',
					informational: true,
					isMetaEdge: true
				}
			},
			{
				data: {
					id: 'meta-deployB-to-nodeX',
					source: 'ns',
					target: 'nodeX',
					name: 'runs-on',
					informational: true,
					isMetaEdge: true
				}
			}
		]);
		markCollapsed(cy, 'ns');

		consolidateCollapsedEdges(cy, cy.getElementById('ns'));
		hideRedundantInformationalEdges(cy);

		expect(visibleEdges(cy)).toEqual(['ns->nodeX [meta-ns-to-nodeX]']);
	});

	it('does not merge edges to different external nodes or opposite directions', () => {
		const cy = mountCy([
			{ data: { id: 'ns' } },
			{ data: { id: 'nodeX' } },
			{ data: { id: 'nodeY' } },
			{ data: { id: 'e1', source: 'ns', target: 'nodeX', name: 'runs-on' } },
			{ data: { id: 'e2', source: 'ns', target: 'nodeY', name: 'runs-on' } },
			{ data: { id: 'e3', source: 'nodeX', target: 'ns', name: 'can-reach' } }
		]);
		markCollapsed(cy, 'ns');

		consolidateCollapsedEdges(cy, cy.getElementById('ns'));

		// Each pair has a single edge - nothing to consolidate; all stay as-is.
		expect(visibleEdges(cy)).toEqual([
			'nodeX->ns [e3]',
			'ns->nodeX [e1]',
			'ns->nodeY [e2]'
		]);
	});
});

describe('reconcileCollapsedEdges', () => {
	it('consolidates every collapsed node even when no per-node event fired', () => {
		// Fresh-load case: the compound is restored as collapsed (class present) but
		// its child edges were never consolidated because no clean aftercollapse fired.
		const cy = mountCy(collapsedNsRunsOn());
		markCollapsed(cy, 'ns');

		// Precondition: children are still separate, un-consolidated edges.
		expect(visibleEdges(cy).length).toBe(3);

		reconcileCollapsedEdges(cy);
		hideRedundantInformationalEdges(cy);

		expect(visibleEdges(cy)).toEqual(['ns->nodeX [meta-ns-to-nodeX]']);
	});

	it('is idempotent (repeat calls do not duplicate the meta-edge)', () => {
		const cy = mountCy(collapsedNsRunsOn());
		markCollapsed(cy, 'ns');

		reconcileCollapsedEdges(cy);
		reconcileCollapsedEdges(cy);
		consolidateCollapsedEdges(cy, cy.getElementById('ns'));

		expect(cy.getElementById('meta-ns-to-nodeX').length).toBe(1);
		expect(visibleEdges(cy)).toEqual(['ns->nodeX [meta-ns-to-nodeX]']);
	});
});
