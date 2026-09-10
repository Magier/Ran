import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import type cytoscape from 'cytoscape';
import type { Node } from '$lib/api/index';
import ListenerBadges from './listener_badges.svelte';

type FakeNodeOptions = { visible?: boolean; exists?: boolean; zoom?: number };

/**
 * Minimal stand-in for the parts of the cytoscape core the overlay reads.
 * Rendered dimensions carry the zoom factor, as cytoscape's own do.
 */
function fakeCy(options: FakeNodeOptions = {}): cytoscape.Core {
	const { visible = true, exists = true, zoom = 1 } = options;
	const node = {
		empty: () => !exists,
		visible: () => visible,
		renderedPosition: () => ({ x: 100, y: 60 }),
		renderedOuterWidth: () => 30 * zoom,
		renderedOuterHeight: () => 30 * zoom
	};
	return {
		getElementById: () => node,
		zoom: () => zoom,
		on: vi.fn(),
		off: vi.fn()
	} as unknown as cytoscape.Core;
}

function listener(protocol: string, port: number) {
	return {
		id: `listener/${protocol}/${port}`,
		kind: 'Listener',
		entry: `${protocol}/${port}`,
		protocol,
		port
	};
}

function c2Node(listeners: unknown[]): Node {
	return {
		id: 'c2/ran',
		entityId: 'c2/ran',
		kind: 'C2',
		name: 'Ran',
		entity: { name: 'Ran', listeners }
	};
}

describe('ListenerBadges', () => {
	it('renders one chip per listening port', () => {
		render(ListenerBadges, { cy: fakeCy(), nodes: [c2Node([listener('tcp', 4444), listener('http', 8080)])] });

		expect(screen.getByText('4444')).toBeInTheDocument();
		expect(screen.getByText('8080')).toBeInTheDocument();
		expect(screen.getByTitle('http/8080')).toBeInTheDocument();
	});

	it('anchors the chips to the top-right of the node', () => {
		const { container } = render(ListenerBadges, {
			cy: fakeCy(),
			nodes: [c2Node([listener('tcp', 4444)])]
		});

		const chips = container.querySelector<HTMLElement>('.listener-badges');
		expect(chips?.style.left).toBe('119px');
		expect(chips?.style.top).toBe('45px');
		expect(chips?.style.scale).toBe('1');
	});

	it('scales the chips with the zoom level and keeps them on the node edge', () => {
		const { container } = render(ListenerBadges, {
			cy: fakeCy({ zoom: 2 }),
			nodes: [c2Node([listener('tcp', 4444)])]
		});

		const chips = container.querySelector<HTMLElement>('.listener-badges');
		expect(chips?.style.scale).toBe('2');
		// Node edge (100 + 60/2) plus the gap, itself scaled.
		expect(chips?.style.left).toBe('138px');
		expect(chips?.style.top).toBe('30px');
	});

	it('collapses ports past the cap into a hoverable +k chip', () => {
		render(ListenerBadges, {
			cy: fakeCy(),
			nodes: [
				c2Node([
					listener('tcp', 1),
					listener('tcp', 2),
					listener('tcp', 3),
					listener('tcp', 4),
					listener('http', 5)
				])
			]
		});

		const overflow = screen.getByText('+2');
		expect(overflow).toBeInTheDocument();
		// One entry per line so the tooltip lists the collapsed listeners.
		expect(overflow.getAttribute('title')).toBe('tcp/4\nhttp/5');
		expect(screen.queryByText('4')).not.toBeInTheDocument();
	});

	it('draws the listener glyph on every chip', () => {
		const { container } = render(ListenerBadges, {
			cy: fakeCy(),
			nodes: [c2Node([listener('tcp', 4444), listener('http', 8080)])]
		});

		expect(container.querySelectorAll('.listener-badge svg')).toHaveLength(2);
	});

	it('drops the port text in compact mode but keeps the glyph and the tooltip', () => {
		const { container } = render(ListenerBadges, {
			cy: fakeCy(),
			nodes: [c2Node([listener('tcp', 4444)])],
			mode: 'icon'
		});

		expect(screen.queryByText('4444')).not.toBeInTheDocument();
		expect(screen.getByTitle('tcp/4444')).toBeInTheDocument();
		expect(container.querySelector('.listener-badge svg')).not.toBeNull();
	});

	it('hides chips while the anchor node is not visible', () => {
		render(ListenerBadges, {
			cy: fakeCy({ visible: false }),
			nodes: [c2Node([listener('tcp', 4444)])]
		});

		expect(screen.queryByText('4444')).not.toBeInTheDocument();
	});

	it('selects the listener entity when a chip is clicked', async () => {
		const onselect = vi.fn();
		render(ListenerBadges, {
			cy: fakeCy(),
			nodes: [c2Node([listener('tcp', 4444), listener('http', 8080)])],
			onselect
		});

		await fireEvent.click(screen.getByText('8080'));

		// The listener's own entity id — that is what scopes the armory to it.
		expect(onselect).toHaveBeenCalledWith('listener/http/8080');
	});

	it('leaves chips inert when nothing handles a click', () => {
		const { container } = render(ListenerBadges, {
			cy: fakeCy(),
			nodes: [c2Node([listener('tcp', 4444)])]
		});

		expect(container.querySelector('.listener-badge.actionable')).toBeNull();
	});

	it('renders nothing before cytoscape is initialized', () => {
		const { container } = render(ListenerBadges, {
			cy: undefined,
			nodes: [c2Node([listener('tcp', 4444)])]
		});

		expect(container.querySelectorAll('.listener-badges')).toHaveLength(0);
	});
});
