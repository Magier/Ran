import { describe, expect, it } from 'vitest';
import cytoscape from 'cytoscape';
// @ts-expect-error cytoscape-elk ships no type declarations
import elk from 'cytoscape-elk';
import { createElkLayout, DEFAULT_LAYOUT_PARAMS } from './elk_layout';

cytoscape.use(elk);

describe('createElkLayout', () => {
	it('uses manual positions as hard final-layout constraints', () => {
		const cy = cytoscape({ headless: true, elements: [{ data: { id: 'manual' } }] });
		const node = cy.getElementById('manual');
		const layout = createElkLayout({}, undefined, { manual: { x: 12, y: 34 } });

		expect(layout.transform?.(node, { x: 900, y: 800 })).toEqual({ x: 12, y: 34 });
	});

	it('leaves untouched nodes under layout control', () => {
		const cy = cytoscape({ headless: true, elements: [{ data: { id: 'automatic' } }] });
		const node = cy.getElementById('automatic');
		const layout = createElkLayout({}, undefined, { manual: { x: 12, y: 34 } });

		expect(layout.transform?.(node, { x: 90, y: 80 })).toEqual({ x: 90, y: 80 });
	});

	it('keeps existing manual nodes fixed during a real ELK run with new nodes', async () => {
		const cy = cytoscape({
			headless: true,
			styleEnabled: true,
			layout: { name: 'preset' },
			elements: [
				{ data: { id: 'left' }, position: { x: 40, y: 60 } },
				{ data: { id: 'right' }, position: { x: 300, y: 140 } },
				{ data: { id: 'discovered' }, position: { x: 100, y: 100 } },
				{ data: { id: 'edge-a', source: 'left', target: 'discovered' } },
				{ data: { id: 'edge-b', source: 'discovered', target: 'right' } }
			]
		});
		const manualPositions = {
			left: { x: 40, y: 60 },
			right: { x: 300, y: 140 }
		};
		const layout = cy
			.elements()
			.layout(
				createElkLayout({}, { ...DEFAULT_LAYOUT_PARAMS, animationDuration: 0 }, manualPositions)
			);
		const stopped = new Promise<void>((resolve) => layout.one('layoutstop', () => resolve()));

		layout.run();
		await stopped;

		expect(cy.getElementById('left').position()).toEqual(manualPositions.left);
		expect(cy.getElementById('right').position()).toEqual(manualPositions.right);
		expect(cy.getElementById('discovered').position()).not.toEqual({ x: 100, y: 100 });
	});
});
