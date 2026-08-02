<script lang="ts">
	import { AccessLevel } from '$lib/model';
	import Icon from '@iconify/svelte';
	import { Tooltip } from '@skeletonlabs/skeleton-svelte';
	import type { TTP, ConsiderationScore } from '$lib/api';
	import ConsiderationBreakdown from './consideration_breakdown.svelte';

	interface ActionCardProps {
		ttp: TTP;
		icon: any;
		enabled?: boolean;
		conditions?: Object;
		onclick: (ttp: TTP) => void;
		class?: string | undefined;
		/** Utility score in [0, 1] for this action against the current target, if scored. */
		utility?: number | undefined;
		/** Per-consideration breakdown behind the score, revealed on hover. */
		breakdown?: ConsiderationScore[] | undefined;
	}

	let {
		ttp,
		icon,
		enabled = true,
		onclick,
		class: className,
		utility = undefined,
		breakdown = undefined
	}: ActionCardProps = $props();

	// Utility bar tint: low → surface, mid → warning, high → success.
	function utilityClass(u: number): string {
		if (u >= 0.66) return 'bg-success-500';
		if (u >= 0.33) return 'bg-warning-500';
		return 'bg-surface-400-600';
	}

	// Determine the icon to use based on TTP name, with specific overrides
	const displayIcon = $derived(() => {
		if (ttp.name.startsWith('Drop ') || ttp.name.startsWith('Install ')) {
			return 'mdi:tray-arrow-down';
		}
		if (ttp.name.startsWith('Execute')) {
			return 'mdi:terminal';
		}
		if (ttp.name.startsWith('Read')) {
			return 'mdi:file-eye';
		}
		if (ttp.name.indexOf('User ID') !== -1) {
			return 'mdi:id-card-outline';
		}
		if (ttp.name.indexOf('IP address') !== -1) {
			return 'mdi:ip-network';
		}
		if (ttp.name.indexOf('Scan') !== -1) {
			return 'mdi:access-point';
		}
		if (ttp.name.indexOf('permissions') !== -1) {
			return 'mdi:key-variant';
		}
		if (ttp.name.indexOf('List Process') !== -1) {
			return 'mdi:application-cog-outline';
		}
		if (ttp.name.indexOf('Files') !== -1) {
			return 'mdi:files';
		}
		if (ttp.name.indexOf('VolumeMount') !== -1) {
			return 'mdi:harddisk';
		}
		return icon;
	});

	let cardStyle = $derived(
		enabled ? 'card-hover bg-surface-300-700' : 'card-disabled bg-surface-50-900-token'
	);

	function checkConditions(ttp: TTP, conditions: Object) {
		// no requirements means the action is always possible
		if (Object.keys(ttp.requires || {}).length == 0) return true;
		if (conditions) {
			for (let [attr, value] of Object.entries(ttp.requires)) {
				if (!conditions.hasOwnProperty(attr)) {
					return false;
				}
				if (attr === 'accessLevel') {
					var requiredLevel = AccessLevel[value];
					var currentLevel = AccessLevel[conditions['accessLevel']];
					if (currentLevel < requiredLevel) {
						return false;
					}
				} else if (attr === 'rbac') {
					let has_capability = false;
					for (let cap of conditions['rbac']) {
						if (cap == value) {
							has_capability = true;
							break;
						}
					}
					if (!has_capability) return false;
				} else if (Array.isArray(value)) {
					let givenValue = conditions[attr].toLowerCase();
					let sat = value.filter((v) => v.toLowerCase() == givenValue);
					if (!sat) {
						return false;
					}
				} else if (conditions[attr] !== value) {
					return false;
				}
			}
		}
		return true;
	}

	function formatRbac(rbac: { verb?: string; resourceType?: string }): string {
		if (rbac.verb && rbac.resourceType) {
			return `${rbac.verb} ${rbac.resourceType}`;
		}
		return '';
	}

	// Check if there are meaningful requirements (excluding kind and accessLevel)
	const hasVisibleRequirements = $derived(() => {
		if (!ttp.requires) return false;
		const filteredEntries = Object.entries(ttp.requires).filter(([name, value]) => {
			// Exclude kind and accessLevel
			if (name === 'kind' || name === 'accessLevel') return false;
			// Exclude rbacPermissions if it's an empty array
			if (name === 'rbacPermissions' && Array.isArray(value) && value.length === 0) return false;
			return true;
		});
		return filteredEntries.length > 0;
	});

	let pct = $derived(utility !== undefined ? Math.round(utility * 100) : 0);
	let activeVeto = $derived(
		breakdown?.find((c) => c.kind === 'utility' && c.veto && c.curved <= 0)
	);
	let beliefBlocker = $derived(
		breakdown?.find((c) => c.kind === 'belief' && c.curved <= 0)
	);
	let scoreBlocker = $derived(activeVeto ?? beliefBlocker);
	let blockerLabel = $derived(
		activeVeto
			? `Vetoed by ${activeVeto.name}`
			: beliefBlocker
				? `Blocked by ${beliefBlocker.name} (belief factor, not a veto)`
				: ''
	);
</script>

<div
	class={[
		cardStyle +
			' flex items-center hover:bg-surface-400-600 text-xs md:text-sm lg:text-base border-surface-50-950',
		className
	]}
	style="overflow: visible;"
>
	<button
		onclick={() => onclick(ttp)}
		class="flex items-center gap-2 flex-1 min-w-0 p-0 pl-4 py-2 text-left bg-transparent"
		role="menuitem"
		tabindex="0"
		disabled={!enabled}
		style="overflow: visible;"
	>
		<Icon icon={displayIcon()} class="inline-block flex-shrink-0" />
		<span class="truncate">{ttp.name}</span>
		{#if hasVisibleRequirements()}
			{#each Object.entries(ttp.requires) as [name, value]}
				{#if !!value}
					{#if name === 'kind'}
						<!-- Skip kind -->
					{:else if name === 'accessLevel'}
						<!-- Skip accessLevel -->
					{:else if name === 'rbacPermissions'}
						{#if Array.isArray(value)}
							{#each value as perms}
								<span
									class="req-badge badge bg-success-100-900 text-secondary-contrast-200-800 cursor-help text-xs"
								>
									<Icon icon={'carbon-user-admin'} width="12" class="inline-block flex-shrink-0"></Icon>
									<span class="req-text">{formatRbac(perms as { verb?: string; resourceType?: string })}</span>
								</span>
							{/each}
						{/if}
					{:else if name === 'otherFields'}
						{#each Object.entries(value) as [fieldName, val]}
							<span
								class="req-badge badge bg-surface-100-900 text-secondary-contrast-200-800 cursor-help text-xs"
							>
								<Icon icon={'mdi:dots-horizontal'} width="12" class="inline-block flex-shrink-0"></Icon>
								<span class="req-text">{fieldName}: {JSON.stringify(val)}</span>
							</span>
						{/each}
					{:else}
						<span
							class="req-badge badge bg-surface-100-900 text-secondary-contrast-200-800 cursor-help text-xs"
						>
							<Icon icon={'mdi:information-outline'} width="12" class="inline-block flex-shrink-0"></Icon>
							<span class="req-text">{name}: {JSON.stringify(value)}</span>
						</span>
					{/if}
				{/if}
			{/each}
		{/if}
	</button>

	{#if utility !== undefined}
		{#snippet chip()}
			{#if scoreBlocker}
				<Icon icon="mdi:alert-circle" width="14" class="text-error-500" aria-label={blockerLabel} />
			{/if}
			<span class="h-1.5 w-10 rounded bg-surface-300-700 overflow-hidden">
				<span class="block h-full {utilityClass(utility)}" style="width: {pct}%"></span>
			</span>
			<span class="text-[10px] tabular-nums w-6 text-right">{pct}</span>
		{/snippet}

		{#if breakdown && breakdown.length}
			<Tooltip
				openDelay={120}
				closeDelay={80}
				positioning={{ placement: 'right', gutter: 8 }}
			>
				<Tooltip.Trigger
					class="flex items-center gap-1 pr-2 pl-1 flex-shrink-0 cursor-help text-surface-500 hover:text-primary-500"
					aria-label={scoreBlocker ? `Utility ${pct} — ${blockerLabel}` : `Utility ${pct} — hover for breakdown`}
					title={blockerLabel || 'Hover for utility breakdown'}
				>
					{@render chip()}
				</Tooltip.Trigger>
				<Tooltip.Positioner>
					<Tooltip.Content
						class="z-[90] w-64 bg-surface-100-900 border border-surface-300-700 rounded shadow-xl p-2"
					>
						<div class="flex items-center justify-between mb-1.5">
							<span class="text-[10px] uppercase tracking-wide text-surface-500">Utility</span>
							<span class="text-xs font-semibold">{pct}</span>
						</div>
						<ConsiderationBreakdown {breakdown} />
					</Tooltip.Content>
				</Tooltip.Positioner>
			</Tooltip>
		{:else}
			<span class="flex items-center gap-1 pr-2 pl-1 flex-shrink-0 text-surface-500" title="Utility {pct}">
				{@render chip()}
			</span>
		{/if}
	{/if}
</div>

<style>
	.card-disabled {
		color: #888;
	}

	.req-badge {
		display: inline-flex;
		align-items: center;
		gap: 0.125rem;
		position: relative;
		white-space: nowrap;
		z-index: 10;
		padding: 0.125rem 0.25rem;
	}

	.req-text {
		opacity: 1;
	}
</style>
