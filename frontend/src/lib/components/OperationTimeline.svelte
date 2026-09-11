<script lang="ts">
	import Icon from '@iconify/svelte';
	import type { TopEntry, EntityEntry, ActionGroup } from '$lib/stores/timelineStore.svelte';

	interface Props {
		entries: TopEntry[];
		onfocusentity: (targetId: string) => void;
		ontogglegroup: (cmdId: string) => void;
		onviewaction: (cmdId: string) => void;
	}

	let { entries, onfocusentity, ontogglegroup, onviewaction }: Props = $props();

	const MIN_HEIGHT = 120;
	const MAX_HEIGHT = 800;
	const DEFAULT_HEIGHT = 240; // matches the previous fixed h-60
	const STORAGE_KEY = 'operationTimeline.height';

	function loadHeight(): number {
		if (typeof localStorage === 'undefined') return DEFAULT_HEIGHT;
		const raw = Number(localStorage.getItem(STORAGE_KEY));
		if (!Number.isFinite(raw) || raw <= 0) return DEFAULT_HEIGHT;
		return Math.min(MAX_HEIGHT, Math.max(MIN_HEIGHT, raw));
	}

	let height = $state(loadHeight());

	function startResize(event: PointerEvent) {
		event.preventDefault();
		const startY = event.clientY;
		const startHeight = height;

		function onMove(e: PointerEvent) {
			// Dragging up (smaller clientY) grows the panel.
			const next = startHeight + (startY - e.clientY);
			height = Math.min(MAX_HEIGHT, Math.max(MIN_HEIGHT, next));
		}
		function onUp() {
			window.removeEventListener('pointermove', onMove);
			window.removeEventListener('pointerup', onUp);
			document.body.style.cursor = '';
			document.body.style.userSelect = '';
			if (typeof localStorage !== 'undefined') {
				localStorage.setItem(STORAGE_KEY, String(Math.round(height)));
			}
		}

		window.addEventListener('pointermove', onMove);
		window.addEventListener('pointerup', onUp);
		document.body.style.cursor = 'row-resize';
		document.body.style.userSelect = 'none';
	}

	function onHandleKeydown(event: KeyboardEvent) {
		const step = event.shiftKey ? 48 : 16;
		if (event.key === 'ArrowUp') {
			height = Math.min(MAX_HEIGHT, height + step);
		} else if (event.key === 'ArrowDown') {
			height = Math.max(MIN_HEIGHT, height - step);
		} else {
			return;
		}
		event.preventDefault();
		if (typeof localStorage !== 'undefined') {
			localStorage.setItem(STORAGE_KEY, String(Math.round(height)));
		}
	}

	function formatCompactTime(d: Date): string {
		return d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', hour12: false });
	}

	function formatFullTime(d: Date): string {
		return d.toLocaleString([], { dateStyle: 'medium', timeStyle: 'long' });
	}

	let timestampTooltip = $state<{ text: string; left: number; top: number }>();

	function showTimestampTooltip(event: MouseEvent | FocusEvent, timestamp: Date) {
		const trigger = event.currentTarget;
		if (!(trigger instanceof HTMLElement)) return;
		const bounds = trigger.getBoundingClientRect();
		timestampTooltip = {
			text: formatFullTime(timestamp),
			left: bounds.left + bounds.width / 2,
			top: bounds.top - 6
		};
	}

	function hideTimestampTooltip() {
		timestampTooltip = undefined;
	}

	/** Lower-cased for prose, but only for the kinds that read naturally that way. */
	const SPELLED_OUT_KINDS: Record<string, string> = {
		Pod: 'pod',
		Namespace: 'namespace',
		ServiceAccount: 'service account',
		Listener: 'listener'
	};

	function entityNoun(entry: EntityEntry): string {
		return SPELLED_OUT_KINDS[entry.entityKind] ?? entry.entityKind;
	}

	function entityPrefix(entry: EntityEntry): string {
		if (entry.kind === 'credential') {
			if (entry.entityKind === 'Secret') return 'Found secret';
			return 'Found credential';
		}
		if (entry.kind === 'access-gained') return 'Gained exec access to';
		// The verb has to match what actually happened. Calling the listener an
		// action just bound "discovered" is the kind of claim that makes the
		// whole timeline harder to trust.
		const noun = entityNoun(entry);
		switch (entry.outcome ?? 'observed') {
			case 'created':
				return `Created ${noun}`;
			case 'updated':
				return `Updated ${noun}`;
			default:
				return `Discovered ${noun}`;
		}
	}

	function effectCounts(group: ActionGroup) {
		const counts = { discovery: 0, created: 0, credential: 0, access: 0 };
		for (const e of group.effects) {
			const outcome = e.outcome ?? 'observed';
			if (e.kind === 'credential') counts.credential++;
			else if (e.kind === 'access-gained') counts.access++;
			else if (outcome === 'created') counts.created++;
			else if (outcome === 'observed') counts.discovery++;
			// 'updated' effects are deliberately uncounted: the badge row
			// summarises what the action yielded, not what it re-stated.
		}
		return counts;
	}

	function entityIcon(entry: EntityEntry): string {
		if (entry.kind === 'credential') return 'mdi:key';
		if (entry.kind === 'access-gained') return 'mdi:shield-check';
		if ((entry.outcome ?? 'observed') === 'created') return 'mdi:plus-circle-outline';
		return 'mdi:magnify';
	}

	function entityIconClass(entry: EntityEntry): string {
		if (entry.kind === 'credential') return 'size-4 text-warning-500';
		if (entry.kind === 'access-gained') return 'size-4 text-success-400';
		if ((entry.outcome ?? 'observed') === 'created') return 'size-4 text-tertiary-400';
		return 'size-4 text-primary-400';
	}

	let totalEvents = $derived(
		entries.reduce((n, e) => {
			if (e.kind === 'action-group') return n + 1 + e.effects.length;
			return n + 1;
		}, 0)
	);

	// The store keeps entries newest-first; render oldest→newest so the latest
	// sits at the bottom, like a log/chat view.
	let ordered = $derived([...entries].reverse());

	let scrollEl: HTMLDivElement | undefined = $state();
	// Pinned to the bottom by default so the newest events stay in view. Flips
	// off once the user scrolls up into the history, and back on when they
	// return to the bottom.
	let stickToBottom = $state(true);

	function onTimelineScroll() {
		if (!scrollEl) return;
		hideTimestampTooltip();
		const slack = 24; // px tolerance - near-bottom still counts as bottom
		stickToBottom = scrollEl.scrollTop + scrollEl.clientHeight >= scrollEl.scrollHeight - slack;
	}

	// Follow new events to the bottom while pinned. Depends on totalEvents so it
	// also fires when an expanded group gains child effect rows, and on scrollEl
	// so the initial (backfilled) list lands at the bottom once mounted.
	$effect(() => {
		void totalEvents;
		if (stickToBottom && scrollEl) {
			scrollEl.scrollTop = scrollEl.scrollHeight;
		}
	});
</script>

{#snippet timestamp(value?: Date, startup = false)}
	{#if value}
		{@const fullTimestamp = formatFullTime(value)}
		<button
			type="button"
			class="text-surface-500 mt-0.5 shrink-0 cursor-help text-xs"
			aria-label={fullTimestamp}
			onmouseenter={(event) => showTimestampTooltip(event, value)}
			onmouseleave={hideTimestampTooltip}
			onfocus={(event) => showTimestampTooltip(event, value)}
			onblur={hideTimestampTooltip}
			onclick={(event) => event.stopPropagation()}
		>
			{startup ? 'Startup' : formatCompactTime(value)}
		</button>
	{:else}
		<span class="text-surface-500 mt-0.5 shrink-0 text-xs">Startup</span>
	{/if}
{/snippet}

<div
	class="bg-surface-100-900 border-surface-200-800 relative flex shrink-0 flex-col border-t"
	style="height: {height}px"
	role="region"
	aria-label="Operation timeline"
>
	<!-- Resize handle -->
	<div
		class="group absolute -top-1 right-0 left-0 z-10 h-2 cursor-row-resize"
		role="slider"
		aria-orientation="horizontal"
		aria-label="Resize operation timeline"
		aria-valuemin={MIN_HEIGHT}
		aria-valuemax={MAX_HEIGHT}
		aria-valuenow={Math.round(height)}
		tabindex="0"
		onpointerdown={startResize}
		onkeydown={onHandleKeydown}
	>
		<div
			class="group-hover:bg-primary-500 absolute inset-x-0 top-1 h-0.5 bg-transparent transition-colors"
		></div>
	</div>

	<!-- Header -->
	<div class="border-surface-200-800 flex shrink-0 items-center border-b px-3 py-1.5">
		<span class="text-sm font-semibold">Operation Timeline</span>
		<span class="text-surface-500 ml-2 text-xs"
			>{totalEvents} event{totalEvents === 1 ? '' : 's'}</span
		>
	</div>

	<!-- Entry list -->
	<div
		class="flex flex-1 flex-col overflow-y-auto"
		bind:this={scrollEl}
		onscroll={onTimelineScroll}
	>
		{#if entries.length === 0}
			<div class="text-surface-500 flex h-full items-center justify-center text-sm">
				No events yet
			</div>
		{:else}
			{#each ordered as entry (entry.kind === 'action-group' ? entry.action.id : entry.id)}
				{#if entry.kind === 'action-group'}
					{@const counts = effectCounts(entry)}
					<!-- Action group header row -->
					<div
						class="border-surface-200-800 hover:bg-surface-200-800 relative flex cursor-pointer items-start gap-2 border-b px-3 py-2 text-sm select-none"
						role="button"
						tabindex="0"
						onclick={() => ontogglegroup(entry.action.id)}
						onkeydown={(event) => {
							if (event.key === 'Enter' || event.key === ' ') ontogglegroup(entry.action.id);
						}}
						aria-expanded={!entry.collapsed}
					>
						<!-- Chevron: sits in the left gutter rather than taking a
                             column, so the status icon stays aligned with the icon
                             of a standalone entity row. -->
						{#if entry.effects.length > 0}
							<Icon
								icon={entry.collapsed ? 'mdi:chevron-right' : 'mdi:chevron-down'}
								class="text-surface-500 absolute top-3 left-0 size-3"
								aria-hidden="true"
							/>
						{/if}

						<!-- Status icon -->
						<div class="mt-0.5 shrink-0">
							{#if entry.action.status === 'pending'}
								<Icon icon="svg-spinners:90-ring-with-bg" class="size-4" aria-hidden="true" />
							{:else if entry.action.status === 'success'}
								<Icon icon="mdi:check-circle" class="text-success-500 size-4" aria-hidden="true" />
							{:else}
								<Icon icon="mdi:close-circle" class="text-error-500 size-4" aria-hidden="true" />
							{/if}
						</div>

						<!-- Label: ttpName on target [via execSystem] -->
						<div class="min-w-0 flex-1">
							<div class="flex flex-wrap items-center gap-1 leading-tight">
								{#if entry.action.startup}
									<span class="font-medium">{entry.action.ttpName}</span>
								{:else}
									<button
										type="button"
										class="text-primary-500 font-medium hover:underline"
										onclick={(event) => {
											event.stopPropagation();
											onviewaction(entry.action.id);
										}}
									>
										{entry.action.ttpName}
									</button>
								{/if}
								{#if entry.action.startup}
									<span class="text-surface-500 text-xs">{entry.action.detail}</span>
								{:else}
									<span class="text-surface-500">on</span>
									<button
										type="button"
										class="text-primary-500 truncate hover:underline"
										title={entry.action.targetName}
										onclick={(e) => {
											e.stopPropagation();
											onfocusentity(entry.action.targetId);
										}}
									>
										{entry.action.targetName}
									</button>
									{#if entry.action.execSystemName}
										<span class="text-surface-500 text-xs">via</span>
										<span
											class="text-surface-500 truncate text-xs"
											title={entry.action.execSystemName}
										>
											{entry.action.execSystemName}
										</span>
									{/if}
								{/if}
							</div>
							{#if entry.action.status === 'failed' && entry.action.failReason}
								<div class="text-error-500 mt-0.5 truncate text-xs" title={entry.action.failReason}>
									{entry.action.failReason}
								</div>
							{/if}
						</div>

						<!-- Effect chips -->
						<div class="mt-0.5 flex shrink-0 items-center gap-1">
							{#if counts.discovery > 0}
								<Icon icon="mdi:magnify" class="text-primary-400 size-3.5" aria-hidden="true" />
								<span class="text-surface-400 text-xs">{counts.discovery}</span>
							{/if}
							{#if counts.created > 0}
								<Icon
									icon="mdi:plus-circle-outline"
									class="text-tertiary-400 size-3.5"
									aria-hidden="true"
								/>
								<span class="text-surface-400 text-xs">{counts.created}</span>
							{/if}
							{#if counts.credential > 0}
								<Icon icon="mdi:key" class="text-warning-500 size-3.5" aria-hidden="true" />
								<span class="text-surface-400 text-xs">{counts.credential}</span>
							{/if}
							{#if counts.access > 0}
								<Icon
									icon="mdi:shield-check"
									class="text-success-400 size-3.5"
									aria-hidden="true"
								/>
								<span class="text-surface-400 text-xs">{counts.access}</span>
							{/if}
							{#if entry.score != null}
								<span class="text-surface-400 ml-1 text-xs">★ {entry.score.toFixed(1)}</span>
							{/if}
						</div>

						<!-- Timestamp -->
						{@render timestamp(entry.action.timestamp, entry.action.startup)}
					</div>

					<!-- Expanded child effect rows -->
					{#if !entry.collapsed}
						{#each entry.effects as effect (effect.id)}
							<div
								class="border-surface-200-800 hover:bg-surface-200-800 border-l-surface-300-700 ml-5 flex items-start gap-2 border-b border-l-2 py-1.5 pr-3 pl-3.5 text-sm"
							>
								<div class="mt-0.5 shrink-0">
									<Icon
										icon={entityIcon(effect)}
										class={entityIconClass(effect)}
										aria-hidden="true"
									/>
								</div>
								<div class="min-w-0 flex-1">
									<span class="font-medium">{entityPrefix(effect)}</span>
									<button
										type="button"
										class="text-primary-500 text-left font-medium hover:underline"
										onclick={() => onfocusentity(effect.entityId)}>{effect.entityName}</button
									>
								</div>
								{@render timestamp(effect.timestamp)}
							</div>
						{/each}
					{/if}
				{:else}
					<!-- Standalone entity row (no parent action) -->
					<div
						class="border-surface-200-800 hover:bg-surface-200-800 flex items-start gap-2 border-b px-3 py-2 text-sm"
					>
						<div class="mt-0.5 shrink-0">
							<Icon icon={entityIcon(entry)} class={entityIconClass(entry)} aria-hidden="true" />
						</div>
						<div class="min-w-0 flex-1">
							<span class="font-medium">{entityPrefix(entry)}</span>
							<button
								type="button"
								class="text-primary-500 text-left font-medium hover:underline"
								onclick={() => onfocusentity(entry.entityId)}>{entry.entityName}</button
							>
						</div>
						{@render timestamp(entry.timestamp)}
					</div>
				{/if}
			{/each}
		{/if}
	</div>

	{#if timestampTooltip}
		<div
			role="tooltip"
			class="bg-surface-100-900 border-surface-300-700 pointer-events-none fixed z-[100] -translate-x-1/2 -translate-y-full rounded border px-2 py-1 text-xs whitespace-nowrap shadow-lg"
			style="left: {timestampTooltip.left}px; top: {timestampTooltip.top}px"
		>
			{timestampTooltip.text}
		</div>
	{/if}
</div>
