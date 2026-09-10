import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import type cytoscape from 'cytoscape';
import type { Node } from '$lib/api/index';
import C2Badges from './c2_badges.svelte';

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

function redirector(playId: string, remotePort: number, listenerPort: number, via = 'labctl') {
	return {
		id: `redirector/${playId}/${remotePort}`,
		kind: 'Redirector',
		entry: `${playId}/${remotePort}`,
		label: `${via} ${remotePort}→${listenerPort}`,
		via,
		playId,
		remotePort,
		listenerPort
	};
}

function c2Node(listeners: unknown[], redirectors: unknown[] = []): Node {
	return {
		id: 'c2/ran',
		entityId: 'c2/ran',
		kind: 'C2',
		name: 'Ran',
		entity: { name: 'Ran', listeners, redirectors }
	};
}

describe('C2Badges', () => {
	it('renders one chip per listening port', () => {
		render(C2Badges, {
			cy: fakeCy(),
			nodes: [c2Node([listener('tcp', 4444), listener('http', 8080)])]
		});

		expect(screen.getByText('4444')).toBeInTheDocument();
		expect(screen.getByText('8080')).toBeInTheDocument();
		expect(screen.getByTitle('http/8080')).toBeInTheDocument();
	});

	it('anchors the chips to the top-right of the node', () => {
		const { container } = render(C2Badges, {
			cy: fakeCy(),
			nodes: [c2Node([listener('tcp', 4444)])]
		});

		const chips = container.querySelector<HTMLElement>('.c2-badges');
		expect(chips?.style.left).toBe('119px');
		expect(chips?.style.top).toBe('45px');
		expect(chips?.style.scale).toBe('1');
	});

	it('scales the chips with the zoom level and keeps them on the node edge', () => {
		const { container } = render(C2Badges, {
			cy: fakeCy({ zoom: 2 }),
			nodes: [c2Node([listener('tcp', 4444)])]
		});

		const chips = container.querySelector<HTMLElement>('.c2-badges');
		expect(chips?.style.scale).toBe('2');
		// Node edge (100 + 60/2) plus the gap, itself scaled.
		expect(chips?.style.left).toBe('138px');
		expect(chips?.style.top).toBe('30px');
	});

	it('collapses ports past the cap into a hoverable +k chip', () => {
		render(C2Badges, {
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
		const { container } = render(C2Badges, {
			cy: fakeCy(),
			nodes: [c2Node([listener('tcp', 4444), listener('http', 8080)])]
		});

		expect(container.querySelectorAll('.badge-part svg')).toHaveLength(2);
	});

	it('drops the port text in compact mode but keeps the glyph and the tooltip', () => {
		const { container } = render(C2Badges, {
			cy: fakeCy(),
			nodes: [c2Node([listener('tcp', 4444)])],
			mode: 'icon'
		});

		expect(screen.queryByText('4444')).not.toBeInTheDocument();
		expect(screen.getByTitle('tcp/4444')).toBeInTheDocument();
		expect(container.querySelector('.badge-part svg')).not.toBeNull();
	});

	it('hides chips while the anchor node is not visible', () => {
		render(C2Badges, {
			cy: fakeCy({ visible: false }),
			nodes: [c2Node([listener('tcp', 4444)])]
		});

		expect(screen.queryByText('4444')).not.toBeInTheDocument();
	});

	it('selects the listener entity when a chip is clicked', async () => {
		const onselect = vi.fn();
		render(C2Badges, {
			cy: fakeCy(),
			nodes: [c2Node([listener('tcp', 4444), listener('http', 8080)])],
			onselect
		});

		await fireEvent.click(screen.getByText('8080'));

		// The listener's own entity id — that is what scopes the armory to it.
		expect(onselect).toHaveBeenCalledWith('listener/http/8080');
	});

	it('leaves chips inert when nothing handles a click', () => {
		const { container } = render(C2Badges, {
			cy: fakeCy(),
			nodes: [c2Node([listener('tcp', 4444)])]
		});

		expect(container.querySelector('.badge-part.actionable')).toBeNull();
	});

	it('renders nothing before cytoscape is initialized', () => {
		const { container } = render(C2Badges, {
			cy: undefined,
			nodes: [c2Node([listener('tcp', 4444)])]
		});

		expect(container.querySelectorAll('.c2-badges')).toHaveLength(0);
	});

	it('draws a redirector as a segment of its listener chip, not a chip of its own', () => {
		const { container } = render(C2Badges, {
			cy: fakeCy(),
			nodes: [c2Node([listener('tcp', 4444)], [redirector('zn1kqxk3ykpvxp5x', 1337, 4444)])]
		});

		// One pill, holding the listener and its adapter as two segments.
		const pills = container.querySelectorAll('.c2-badge');
		expect(pills).toHaveLength(1);
		expect(pills[0].querySelectorAll('.badge-part')).toHaveLength(2);
		expect(pills[0].querySelector('.badge-part.adapter')).not.toBeNull();

		expect(screen.getByText('4444')).toBeInTheDocument();
		expect(screen.getByText('1337')).toBeInTheDocument();
		// The tooltip names the tool, because that is what says which kind of
		// redirector this is; the playground id alone carries nothing.
		expect(
			screen.getByTitle(
				'labctl 1337→4444 — via labctl on playground zn1kqxk3ykpvxp5x'
			)
		).toBeInTheDocument();
	});

	it('selects the redirector entity when its segment is clicked', async () => {
		const onselect = vi.fn();
		render(C2Badges, {
			cy: fakeCy(),
			nodes: [c2Node([listener('tcp', 4444)], [redirector('play1', 1337, 4444)])],
			onselect
		});

		await fireEvent.click(screen.getByText('1337'));

		expect(onselect).toHaveBeenCalledWith('redirector/play1/1337');
	});

	it('keeps the listener selectable through its own segment', async () => {
		const onselect = vi.fn();
		render(C2Badges, {
			cy: fakeCy(),
			nodes: [c2Node([listener('tcp', 4444)], [redirector('play1', 1337, 4444)])],
			onselect
		});

		await fireEvent.click(screen.getByText('4444'));

		expect(onselect).toHaveBeenCalledWith('listener/tcp/4444');
	});

	it('draws an adapter segment per redirector on the same listener', () => {
		const { container } = render(C2Badges, {
			cy: fakeCy(),
			nodes: [
				c2Node(
					[listener('tcp', 4444)],
					[redirector('play1', 1337, 4444), redirector('play2', 1338, 4444)]
				)
			]
		});

		expect(container.querySelectorAll('.c2-badge')).toHaveLength(1);
		expect(container.querySelectorAll('.badge-part.adapter')).toHaveLength(2);
	});

	it('draws a redirector whose listener is gone as its own open-ended pill', () => {
		const { container } = render(C2Badges, {
			cy: fakeCy(),
			nodes: [c2Node([listener('tcp', 4444)], [redirector('play1', 1337, 9999)])]
		});

		const orphan = container.querySelector('.c2-badge.orphaned');
		expect(orphan).not.toBeNull();
		// Still reachable, so "Stop Redirector" is still an option.
		expect(orphan?.querySelector('.badge-part')).not.toBeNull();
		expect(screen.getByText('1337')).toBeInTheDocument();
	});
});
