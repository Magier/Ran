import { describe, expect, it } from 'vitest';
import cytoscape from 'cytoscape';
import {
	captureManualPosition,
	fixedPositionsForLayout,
	restoreManualPositions
} from './graph_positions';
import type { PosMap } from './graph_nodes';

function compoundGraph() {
	return cytoscape({
		headless: true,
		styleEnabled: true,
		layout: { name: 'preset' },
		elements: [
			{ data: { id: 'namespace' } },
			{ data: { id: 'pod-a', parent: 'namespace' }, position: { x: 10, y: 20 } },
			{ data: { id: 'pod-b', parent: 'namespace' }, position: { x: 50, y: 60 } },
			{ data: { id: 'outside' }, position: { x: 100, y: 100 } }
		]
	});
}

describe('manual graph positions', () => {
	it('records only the node the user explicitly dragged', () => {
		const cy = compoundGraph();
		const positions: PosMap = {};

		captureManualPosition(cy.getElementById('namespace'), positions);

		expect(Object.keys(positions)).toEqual(['namespace']);
	});

	it('turns a compound anchor into leaf constraints for ELK', () => {
		const cy = compoundGraph();
		const positions: PosMap = { namespace: cy.getElementById('namespace').position() };

		const fixedPositions = fixedPositionsForLayout(cy, positions);

		expect(Object.keys(fixedPositions).sort()).toEqual(['namespace', 'pod-a', 'pod-b']);
		expect(fixedPositions).not.toHaveProperty('outside');
	});

	it('restores explicit child coordinates after an automatic layout moves the hierarchy', () => {
		const cy = compoundGraph();
		const positions: PosMap = {};
		captureManualPosition(cy.getElementById('pod-a'), positions);
		captureManualPosition(cy.getElementById('pod-b'), positions);

		cy.getElementById('namespace').position({ x: 400, y: 400 });
		restoreManualPositions(cy, positions);

		expect(cy.getElementById('pod-a').position()).toEqual(positions['pod-a']);
		expect(cy.getElementById('pod-b').position()).toEqual(positions['pod-b']);
	});

	it('keeps an explicitly moved compound as the anchor after it gains a child', () => {
		const cy = compoundGraph();
		const positions: PosMap = {};
		captureManualPosition(cy.getElementById('namespace'), positions);
		cy.add({
			data: { id: 'pod-new', parent: 'namespace' },
			position: { x: 800, y: 600 }
		});
		expect(cy.getElementById('namespace').position()).not.toEqual(positions.namespace);

		restoreManualPositions(cy, positions);

		expect(cy.getElementById('namespace').position()).toEqual(positions.namespace);
	});

	it('ignores positions for nodes no longer in the graph', () => {
		const cy = compoundGraph();

		expect(() => restoreManualPositions(cy, { missing: { x: 20, y: 30 } })).not.toThrow();
	});
});
