<script lang="ts">
	import { onDestroy } from 'svelte';
	import IconEar from '~icons/emojione-monotone/ear';
	import IconLeftArrow from '~icons/emojione-monotone/left-arrow';
	import type cytoscape from 'cytoscape';
	import type { Node } from '$lib/api/index';
	import type { Redirector } from '$lib/redirectors';
	import { c2Badges, type C2BadgeGroup } from './c2_badges';

	type C2BadgesProps = {
		cy: cytoscape.Core | undefined;
		nodes: Node[] | undefined;
		/**
		 * `port` labels every chip with its ports. `icon` drops the text and
		 * leaves the glyphs alone — the compact form for a dense graph.
		 */
		mode?: 'port' | 'icon';
		/**
		 * Clicking a segment selects that listener or redirector, so the armory
		 * scopes itself to the actions that target it.
		 */
		onselect?: (entityId: string) => void;
	};

	type PlacedGroup = C2BadgeGroup & { x: number; y: number; scale: number };

	// Gap between the node's right edge and the chip stack, in graph units. Like
	// every other chip dimension it is authored at zoom 1 and scaled from there.
	const NODE_GAP = 4;

	let { cy, nodes, mode = 'port', onselect }: C2BadgesProps = $props();

	const groups = $derived(c2Badges(nodes));
	let placed = $state<PlacedGroup[]>([]);
	let frame = 0;

	function adapterTitle(adapter: Redirector): string {
		// Lead with the tool: it is what says which kind of redirector this is,
		// and the playground id on its own carries nothing.
		const hop = `${adapter.label} — via ${adapter.via} on playground ${adapter.playId}`;
		return onselect ? `${hop}; click for actions` : hop;
	}

	function orphanTitle(orphan: Redirector): string {
		const hop = `${orphan.label} — via ${orphan.via} on playground ${orphan.playId}, forwarding to port ${orphan.listenerPort}, which no listener holds any more`;
		return onselect ? `${hop}; click for actions` : hop;
	}

	function place(groupList: C2BadgeGroup[], core: cytoscape.Core): PlacedGroup[] {
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

	function sync(core: cytoscape.Core, groupList: C2BadgeGroup[]) {
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

<div class="c2-badge-layer" aria-hidden={placed.length === 0}>
	{#each placed as group (group.nodeId)}
		<div class="c2-badges" style="left: {group.x}px; top: {group.y}px; scale: {group.scale}">
			{#each group.listeners.visible as badge (badge.listener.id)}
				<!--
					One pill per listener. Its redirectors are segments of the same pill
					rather than chips of their own, because a redirector is a remote entry
					point onto this listener, not a peer of it.
				-->
				<div class="c2-badge" class:compact={mode === 'icon'}>
					<button
						type="button"
						class="badge-part"
						class:actionable={!!onselect}
						title={onselect
							? `${badge.listener.entry} — actions for this listener`
							: badge.listener.entry}
						onclick={() => onselect?.(badge.listener.id)}
					>
						<IconEar />
						{#if mode === 'port'}{badge.listener.port}{/if}
					</button>
					{#each badge.adapters as adapter (adapter.id)}
						<button
							type="button"
							class="badge-part adapter"
							class:actionable={!!onselect}
							title={adapterTitle(adapter)}
							onclick={() => onselect?.(adapter.id)}
						>
							<IconLeftArrow />
							{#if mode === 'port'}{adapter.remotePort}{/if}
						</button>
					{/each}
				</div>
			{/each}
			{#if group.listeners.overflowCount > 0}
				<span class="c2-badge overflow" title={group.listeners.overflowTitle}>
					+{group.listeners.overflowCount}
				</span>
			{/if}
			{#each group.orphans.visible as orphan (orphan.id)}
				<!--
					A redirector whose listener is gone has nothing to attach to, but its
					tunnel is still up — so it gets a pill of its own and stays stoppable.
				-->
				<div class="c2-badge orphaned" class:compact={mode === 'icon'}>
					<button
						type="button"
						class="badge-part adapter"
						class:actionable={!!onselect}
						title={orphanTitle(orphan)}
						onclick={() => onselect?.(orphan.id)}
					>
						<IconLeftArrow />
						{#if mode === 'port'}{orphan.remotePort}{/if}
					</button>
				</div>
			{/each}
			{#if group.orphans.overflowCount > 0}
				<span class="c2-badge overflow" title={group.orphans.overflowTitle}>
					+{group.orphans.overflowCount}
				</span>
			{/if}
		</div>
	{/each}
</div>

<style>
	.c2-badge-layer {
		position: absolute;
		inset: 0;
		overflow: hidden;
		pointer-events: none;
		z-index: 1;
	}

	.c2-badges {
		position: absolute;
		display: flex;
		flex-direction: column;
		align-items: flex-start;
		gap: 2px;
		/* Scaling is anchored to the node edge the stack hangs off. */
		transform-origin: top left;
	}

	/* The pill. It owns the outline so a listener and its redirectors read as one
	   object; the segments inside are only click targets. */
	.c2-badge {
		display: inline-flex;
		align-items: stretch;
		height: 12px;
		border: 1px solid color-mix(in srgb, currentColor 25%, transparent);
		border-radius: 6px;
		background-color: var(--color-surface-100-900, rgb(255 255 255 / 0.85));
		color: var(--color-surface-contrast-100-900, inherit);
		font-size: 8px;
		font-variant-numeric: tabular-nums;
		line-height: 1;
		white-space: nowrap;
		overflow: hidden;
	}

	/* An orphaned redirector is still live but no longer wired to anything, so it
	   is drawn as an open-ended pill rather than a complete one. */
	.c2-badge.orphaned {
		border-style: dashed;
	}

	.badge-part {
		/* The segments are <button>, so reset the UA styling the pill replaces. */
		appearance: none;
		margin: 0;
		border: 0;
		background: none;
		font-family: inherit;
		font-size: inherit;
		font-variant-numeric: inherit;
		line-height: inherit;
		color: inherit;
		display: inline-flex;
		align-items: center;
		gap: 2px;
		padding: 0 3px;
		pointer-events: auto;
	}

	.c2-badge.compact .badge-part {
		padding: 0 2px;
	}

	/* The divider is what makes the adapter read as attached to the listener
	   rather than as a separate chip that happens to sit alongside it. */
	.badge-part.adapter {
		border-left: 1px solid color-mix(in srgb, currentColor 25%, transparent);
	}

	.c2-badge.orphaned .badge-part.adapter {
		border-left: 0;
	}

	.badge-part.actionable {
		cursor: pointer;
	}

	.badge-part.actionable:hover,
	.badge-part.actionable:focus-visible {
		background-color: var(--color-surface-200-800, rgb(255 255 255 / 0.95));
	}

	/* The icon is a child component, so its svg needs a global selector — kept
	   inside .badge-part so the rule cannot escape the chip. Monotone art means
	   it picks up the chip's own color. */
	.badge-part :global(svg) {
		display: block;
		width: 8px;
		height: 8px;
	}

	.c2-badge.overflow {
		align-items: center;
		padding: 0 3px;
		font-weight: 600;
	}
</style>
