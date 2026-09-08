import { describe, expect, it, beforeAll } from 'vitest';
import cytoscape from 'cytoscape';
// @ts-ignore
import expandCollapse from 'cytoscape-expand-collapse';
import {
	consolidateCollapsedEdges,
	restoreConsolidatedEdges,
	reconcileCollapsedEdges,
	hideRedundantInformationalEdges,
	COLLAPSED_EDGE_CLASS
} from './graph_edges';

cytoscape.use(expandCollapse);

// Stub canvas so the expand-collapse plugin can initialize under jsdom.
beforeAll(() => {
	const proto = window.HTMLCanvasElement.prototype as any;
	proto.getContext = () => ({
		clearRect() {},
		save() {},
		restore() {},
		beginPath() {},
		moveTo() {},
		lineTo() {},
		stroke() {},
		fill() {},
		arc() {},
		translate() {},
		scale() {},
		rotate() {},
		closePath() {},
		rect() {},
		setTransform() {},
		drawImage() {},
		putImageData() {},
		createImageData() {},
		getImageData() {
			return { data: [] };
		},
		measureText() {
			return { width: 0 };
		},
		fillText() {},
		fillRect() {},
		setLineDash() {},
		quadraticCurveTo() {},
		bezierCurveTo() {},
		set fillStyle(_v: any) {},
		get fillStyle() {
			return '';
		},
		set strokeStyle(_v: any) {},
		get strokeStyle() {
			return '';
		}
	});
});

function mountCy(elements: any[]) {
	const container = document.createElement('div');
	container.getBoundingClientRect = () =>
		({ width: 800, height: 600, top: 0, left: 0, right: 800, bottom: 600, x: 0, y: 0 }) as any;
	Object.defineProperty(container, 'offsetWidth', { value: 800 });
	Object.defineProperty(container, 'offsetHeight', { value: 600 });
	Object.defineProperty(container, 'clientWidth', { value: 800 });
	Object.defineProperty(container, 'clientHeight', { value: 600 });
	document.body.appendChild(container);
	return cytoscape({ container, elements, styleEnabled: true, layout: { name: 'preset' } });
}

function initExpandCollapse(cy: any) {
	const api = cy.expandCollapse({
		layoutBy: null,
		animate: false,
		undoable: false,
		edgeTypeInfo: 'name',
		groupEdgesOfSameTypeOnCollapse: true
	});
	// Wire the same handlers graph.svelte uses.
	cy.on('expandcollapse.aftercollapse', (evt: any) => consolidateCollapsedEdges(cy, evt.target));
	cy.on('expandcollapse.afterexpand', (evt: any) => {
		restoreConsolidatedEdges(cy, evt.target);
		hideRedundantInformationalEdges(cy);
	});
	return api;
}

function visibleEdges(cy: any): string[] {
	return cy
		.edges()
		.filter((e: any) => e.visible())
		.map((e: any) => `${e.source().id()}->${e.target().id()} [${e.id()}]`)
		.sort();
}

function nsWithPods() {
	return [
		{ data: { id: 'ns' } },
		{ data: { id: 'pod1', parent: 'ns' } },
		{ data: { id: 'pod2', parent: 'ns' } },
		{ data: { id: 'pod3', parent: 'ns' } },
		{ data: { id: 'nodeX' } },
		{ data: { id: 'e1', source: 'pod1', target: 'nodeX', name: 'runs-on', informational: true } },
		{ data: { id: 'e2', source: 'pod2', target: 'nodeX', name: 'runs-on', informational: true } },
		{ data: { id: 'e3', source: 'pod3', target: 'nodeX', name: 'runs-on', informational: true } }
	];
}

describe('consolidateCollapsedEdges', () => {
	it('collapses parallel same-type edges to the same external node into one meta-edge', () => {
		const cy = mountCy(nsWithPods());
		const api = initExpandCollapse(cy);

		api.collapse(cy.getElementById('ns'));

		expect(visibleEdges(cy)).toEqual(['ns->nodeX [meta-ns-to-nodeX]']);
	});

	it('keeps the collapsed edges hidden when informational filtering re-runs', () => {
		const cy = mountCy(nsWithPods());
		const api = initExpandCollapse(cy);

		api.collapse(cy.getElementById('ns'));
		// The reactive effect re-applies informational filtering after every update.
		hideRedundantInformationalEdges(cy);

		// Regression: previously this re-showed all three per-pod runs-on children.
		expect(visibleEdges(cy)).toEqual(['ns->nodeX [meta-ns-to-nodeX]']);
	});

	it('tags consolidated children with the collapse marker class', () => {
		const cy = mountCy(nsWithPods());
		const api = initExpandCollapse(cy);
		api.collapse(cy.getElementById('ns'));

		['e1', 'e2', 'e3'].forEach((id) => {
			expect(cy.getElementById(id).hasClass(COLLAPSED_EDGE_CLASS)).toBe(true);
		});
	});

	it('inherits informational styling when all consolidated edges are informational', () => {
		const cy = mountCy(nsWithPods());
		const api = initExpandCollapse(cy);
		api.collapse(cy.getElementById('ns'));
		// The informational filter also re-runs in the app; make sure it does not
		// hide the meta-edge and the flag survives.
		hideRedundantInformationalEdges(cy);

		const meta = cy.getElementById('meta-ns-to-nodeX');
		expect(meta.length).toBe(1);
		expect(meta.data('informational')).toBe(true);
		expect(meta.visible()).toBe(true);
	});

	it('does NOT mark the meta-edge informational when any child is actionable', () => {
		const cy = mountCy([
			{ data: { id: 'ns' } },
			{ data: { id: 'pod1', parent: 'ns' } },
			{ data: { id: 'pod2', parent: 'ns' } },
			{ data: { id: 'nodeX' } },
			// runs-on is informational, exploits is actionable — same directed pair
			{ data: { id: 'e1', source: 'pod1', target: 'nodeX', name: 'runs-on', informational: true } },
			{ data: { id: 'e2', source: 'pod2', target: 'nodeX', name: 'exploits' } }
		]);
		const api = initExpandCollapse(cy);
		api.collapse(cy.getElementById('ns'));

		const meta = cy.getElementById('meta-ns-to-nodeX');
		expect(meta.length).toBe(1);
		expect(meta.data('informational')).toBe(false);
	});

	it('restores the original edges and clears the marker on expand', () => {
		const cy = mountCy(nsWithPods());
		const api = initExpandCollapse(cy);
		api.collapse(cy.getElementById('ns'));

		// Exercise the restore path directly. Driving api.expand() is impossible
		// under jsdom because the plugin's expand renderer path needs a real canvas.
		restoreConsolidatedEdges(cy, cy.getElementById('ns'));

		// Meta-edge gone; original edges restored and the collapse marker cleared.
		expect(cy.getElementById('meta-ns-to-nodeX').length).toBe(0);
		['e1', 'e2', 'e3'].forEach((id) => {
			expect(cy.getElementById(id).hasClass(COLLAPSED_EDGE_CLASS)).toBe(false);
		});
	});

	it('collapses parallel edges across nested workload compounds (ns > deployment > pods)', () => {
		const cy = mountCy([
			{ data: { id: 'ns' } },
			{ data: { id: 'deployA', parent: 'ns' } },
			{ data: { id: 'podA1', parent: 'deployA' } },
			{ data: { id: 'podA2', parent: 'deployA' } },
			{ data: { id: 'deployB', parent: 'ns' } },
			{ data: { id: 'podB1', parent: 'deployB' } },
			{ data: { id: 'podB2', parent: 'deployB' } },
			{ data: { id: 'nodeX' } },
			{
				data: { id: 'a1', source: 'podA1', target: 'nodeX', name: 'runs-on', informational: true }
			},
			{
				data: { id: 'a2', source: 'podA2', target: 'nodeX', name: 'runs-on', informational: true }
			},
			{
				data: { id: 'b1', source: 'podB1', target: 'nodeX', name: 'runs-on', informational: true }
			},
			{ data: { id: 'b2', source: 'podB2', target: 'nodeX', name: 'runs-on', informational: true } }
		]);
		const api = initExpandCollapse(cy);

		// Workload compounds start collapsed, then the namespace is collapsed.
		api.collapse(cy.getElementById('deployA'));
		api.collapse(cy.getElementById('deployB'));
		api.collapse(cy.getElementById('ns'));
		hideRedundantInformationalEdges(cy);

		expect(visibleEdges(cy)).toEqual(['ns->nodeX [meta-ns-to-nodeX]']);
	});

	it('does not merge edges to different external nodes or opposite directions', () => {
		const cy = mountCy([
			{ data: { id: 'ns' } },
			{ data: { id: 'pod1', parent: 'ns' } },
			{ data: { id: 'pod2', parent: 'ns' } },
			{ data: { id: 'nodeX' } },
			{ data: { id: 'nodeY' } },
			{ data: { id: 'e1', source: 'pod1', target: 'nodeX', name: 'runs-on' } },
			{ data: { id: 'e2', source: 'pod2', target: 'nodeY', name: 'runs-on' } },
			{ data: { id: 'e3', source: 'nodeX', target: 'pod1', name: 'can-reach' } }
		]);
		const api = initExpandCollapse(cy);
		api.collapse(cy.getElementById('ns'));

		// Distinct pairs stay distinct: ns->nodeX, ns->nodeY, nodeX->ns (sorted).
		expect(visibleEdges(cy)).toEqual(['nodeX->ns [e3]', 'ns->nodeX [e1]', 'ns->nodeY [e2]']);
	});

	it('MOUNT SIM: collapsing on load (animate:true, restore path) consolidates children', () => {
		const cy = mountCy(nsWithPods());
		// Match the app's real plugin config, including animate:true and fisheye:false.
		const api = (cy as any).expandCollapse({
			layoutBy: null,
			fisheye: false,
			animate: true,
			animationDuration: 300,
			undoable: false,
			edgeTypeInfo: 'name',
			groupEdgesOfSameTypeOnCollapse: true
		});
		cy.on('expandcollapse.aftercollapse', (evt: any) => consolidateCollapsedEdges(cy, evt.target));
		cy.on('expandcollapse.afterexpand', (evt: any) => {
			restoreConsolidatedEdges(cy, evt.target);
			hideRedundantInformationalEdges(cy);
		});

		// Simulate the mount effect: node is restored as collapsed from persisted
		// state, i.e. graph.svelte's recollapseNodes() runs ecApi.collapse(node).
		api.collapse(cy.getElementById('ns'));

		// Then the post-collapse passes the update effect runs.
		hideRedundantInformationalEdges(cy);

		expect(visibleEdges(cy)).toEqual(['ns->nodeX [meta-ns-to-nodeX]']);
	});

	it('reconcileCollapsedEdges consolidates a collapsed node even if no event fired', () => {
		const cy = mountCy(nsWithPods());
		// Init the plugin but DO NOT wire the aftercollapse handler — this mimics the
		// mount case where the event does not cleanly drive consolidation.
		const api = (cy as any).expandCollapse({
			layoutBy: null,
			animate: false,
			undoable: false,
			edgeTypeInfo: 'name',
			groupEdgesOfSameTypeOnCollapse: true
		});
		api.collapse(cy.getElementById('ns'));

		// Without a handler, the children are still present as separate edges.
		expect(visibleEdges(cy).length).toBeGreaterThan(1);

		// The safety-net pass fixes it.
		reconcileCollapsedEdges(cy);
		hideRedundantInformationalEdges(cy);
		expect(visibleEdges(cy)).toEqual(['ns->nodeX [meta-ns-to-nodeX]']);
	});

	it('consolidateCollapsedEdges is idempotent (double call does not duplicate)', () => {
		const cy = mountCy(nsWithPods());
		const api = initExpandCollapse(cy);
		api.collapse(cy.getElementById('ns'));
		// Call again directly — should be a no-op, not create a second meta-edge
		// or re-hide/duplicate anything.
		consolidateCollapsedEdges(cy, cy.getElementById('ns'));
		reconcileCollapsedEdges(cy);

		expect(cy.getElementById('meta-ns-to-nodeX').length).toBe(1);
		expect(visibleEdges(cy)).toEqual(['ns->nodeX [meta-ns-to-nodeX]']);
	});
});
