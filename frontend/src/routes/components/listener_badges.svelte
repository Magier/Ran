<script lang="ts">
	import { onDestroy } from 'svelte';
	import IconEar from '~icons/emojione-monotone/ear';
	import type cytoscape from 'cytoscape';
	import type { Node } from '$lib/api/index';
	import { c2ListenerBadges, type ListenerBadgeGroup } from './listener_badges';

	type ListenerBadgesProps = {
		cy: cytoscape.Core | undefined;
		nodes: Node[] | undefined;
		/**
		 * `port` labels every chip with its port number. `icon` drops the text and
		 * leaves the ear glyph alone — the compact form for a dense graph.
		 */
		mode?: 'port' | 'icon';
		/**
		 * Clicking a chip selects that listener, so the armory scopes itself to
		 * the actions that target it.
		 */
		onselect?: (listenerId: string) => void;
	};

	type PlacedGroup = ListenerBadgeGroup & { x: number; y: number; scale: number };

	// Gap between the node's right edge and the chip stack, in graph units. Like
	// every other chip dimension it is authored at zoom 1 and scaled from there.
	const NODE_GAP = 4;

	let { cy, nodes, mode = 'port', onselect }: ListenerBadgesProps = $props();

	const groups = $derived(c2ListenerBadges(nodes));
	let placed = $state<PlacedGroup[]>([]);
	let frame = 0;

	function place(groupList: ListenerBadgeGroup[], core: cytoscape.Core): PlacedGroup[] {
		const result: PlacedGroup[] = [];
		for (const group of groupList) {
			const node = core.getElementById(group.nodeId);
			// A collapsed compound or a filtered namespace hides the anchor; chips
			// must disappear with it rather than float over empty canvas.
			if (node.empty() || !node.visible()) continue;

			// Rendered dimensions are already screen pixels; the chips themselves are
			// authored at zoom 1 and scaled, so they grow with the node they mark.
			const zoom = core.zoom();
			const position = node.renderedPosition();
			result.push({
				...group,
				x: position.x + node.renderedOuterWidth() / 2 + NODE_GAP * zoom,
				y: position.y - node.renderedOuterHeight() / 2,
				scale: zoom
			});
		}
		return result;
	}

	function sync(core: cytoscape.Core, groupList: ListenerBadgeGroup[]) {
		const next = place(groupList, core);
		// Cytoscape redraws on every animation frame while panning; only touching
		// state on an actual geometry change keeps the overlay off that hot path.
		if (JSON.stringify(next) !== JSON.stringify(placed)) {
			placed = next;
		}
	}

	// Re-place on graph data changes and on anything cytoscape redraws for
	// (pan, zoom, layout, drag, expand/collapse) — `render` covers them all.
	$effect(() => {
		const core = cy;
		const current = groups;
		if (!core) {
			placed = [];
			return;
		}

		const scheduleSync = () => {
			if (frame) return;
			frame = requestAnimationFrame(() => {
				frame = 0;
				sync(core, current);
			});
		};

		sync(core, current);
		core.on('render', scheduleSync);
		return () => {
			core.off('render', scheduleSync);
			if (frame) {
				cancelAnimationFrame(frame);
				frame = 0;
			}
		};
	});

	onDestroy(() => {
		if (frame) cancelAnimationFrame(frame);
	});
</script>

<div class="listener-badge-layer" aria-hidden={placed.length === 0}>
	{#each placed as group (group.nodeId)}
		<div
			class="listener-badges"
			style="left: {group.x}px; top: {group.y}px; scale: {group.scale}"
		>
			{#each group.visible as badge (badge.id)}
				<button
					type="button"
					class="listener-badge"
					class:compact={mode === 'icon'}
					class:actionable={!!onselect}
					title={onselect ? `${badge.entry} — actions for this listener` : badge.entry}
					onclick={() => onselect?.(badge.id)}
				>
					<IconEar />
					{#if mode === 'port'}{badge.port}{/if}
				</button>
			{/each}
			{#if group.overflowCount > 0}
				<span class="listener-badge overflow" title={group.overflowTitle}>
					+{group.overflowCount}
				</span>
			{/if}
		</div>
	{/each}
</div>

<style>
	.listener-badge-layer {
		position: absolute;
		inset: 0;
		overflow: hidden;
		pointer-events: none;
		z-index: 1;
	}

	.listener-badges {
		position: absolute;
		display: flex;
		flex-direction: column;
		align-items: flex-start;
		gap: 2px;
		/* Scaling is anchored to the node edge the stack hangs off. */
		transform-origin: top left;
	}

	.listener-badge {
		/* The chips are <button> when clickable, so reset the UA styling the
		   layer's own look replaces. */
		appearance: none;
		margin: 0;
		font-family: inherit;
		display: inline-flex;
		align-items: center;
		gap: 2px;
		padding: 0 3px;
		height: 12px;
		border: 1px solid color-mix(in srgb, currentColor 25%, transparent);
		border-radius: 6px;
		background-color: var(--color-surface-100-900, rgb(255 255 255 / 0.85));
		color: var(--color-surface-contrast-100-900, inherit);
		font-size: 8px;
		font-variant-numeric: tabular-nums;
		line-height: 1;
		white-space: nowrap;
		pointer-events: auto;
	}

	.listener-badge.compact {
		padding: 0 2px;
	}

	.listener-badge.actionable {
		cursor: pointer;
	}

	.listener-badge.actionable:hover,
	.listener-badge.actionable:focus-visible {
		border-color: currentColor;
		background-color: var(--color-surface-200-800, rgb(255 255 255 / 0.95));
	}

	/* The icon is a child component, so its svg needs a global selector — kept
	   inside .listener-badge so the rule cannot escape the chip. Monotone art
	   means it picks up the chip's own color. */
	.listener-badge :global(svg) {
		display: block;
		width: 8px;
		height: 8px;
	}

	.listener-badge.overflow {
		font-weight: 600;
	}
</style>
