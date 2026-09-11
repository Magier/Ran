<script lang="ts">
	import Icon from '@iconify/svelte';
	import { Tooltip } from '@skeletonlabs/skeleton-svelte';
	import type { TTP, ConsiderationScore } from '$lib/api';
	import ConsiderationBreakdown from './consideration_breakdown.svelte';

	interface ActionCardProps {
		ttp: TTP;
		icon: any;
		enabled?: boolean;
		/** Whether the selected target currently satisfies this TTP's prerequisites. */
		prerequisitesFulfilled?: boolean;
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
		prerequisitesFulfilled = true,
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

	type RequirementDetail = { label: string; value: string };

	function words(name: string): string {
		return name.replace(/([a-z])([A-Z])/g, '$1 $2').replace(/^./, (c) => c.toUpperCase());
	}

	function compactValue(value: unknown): string {
		if (typeof value === 'string' || typeof value === 'number') return String(value);
		if (typeof value === 'boolean') return value ? 'Required' : 'Not required';
		if (Array.isArray(value)) return value.map(compactValue).join(', ');
		if (value && typeof value === 'object') {
			return Object.entries(value)
				.filter(([key]) => key !== 'kubetier')
				.map(([key, item]) => `${words(key)}: ${compactValue(item)}`)
				.join(' · ');
		}
		return String(value);
	}

	function rbacValue(value: unknown): string {
		if (!value || typeof value !== 'object' || Array.isArray(value)) return compactValue(value);
		const permission = value as Record<string, unknown>;
		const action = [permission.verb, permission.resourceType ?? permission.resource]
			.filter(Boolean)
			.join(' ');
		const qualifiers = [permission.resourceName, permission.scope]
			.filter((item) => typeof item === 'string' && item.length > 0)
			.join(' · ');
		return qualifiers ? `${action} · ${qualifiers}` : action || compactValue(value);
	}

	let requirementDetails: RequirementDetail[] = $derived.by(() => {
		const details: RequirementDetail[] = [];
		for (const [name, value] of Object.entries(ttp.requires ?? {})) {
			if (value === undefined || value === null || value === false) continue;
			if (Array.isArray(value) && value.length === 0) continue;

			if (name === 'rbacPermissions' && Array.isArray(value)) {
				for (const permission of value) {
					details.push({ label: 'RBAC', value: rbacValue(permission) });
				}
			} else {
				details.push({ label: words(name), value: compactValue(value) });
			}
		}
		return details;
	});

	let pct = $derived(utility !== undefined ? Math.round(utility * 100) : 0);
	let activeVeto = $derived(
		breakdown?.find((c) => c.kind === 'utility' && c.veto && c.curved <= 0)
	);
	let beliefBlocker = $derived(breakdown?.find((c) => c.kind === 'belief' && c.curved <= 0));
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
			' hover:bg-surface-400-600 border-surface-50-950 flex items-center text-xs md:text-sm lg:text-base',
		className
	]}
	style="overflow: visible;"
>
	<button
		onclick={() => onclick(ttp)}
		class="flex min-w-0 flex-1 items-center gap-2 bg-transparent p-0 py-2 pl-4 text-left"
		role="menuitem"
		tabindex="0"
		disabled={!enabled}
		style="overflow: visible;"
	>
		<Icon icon={displayIcon()} class="inline-block flex-shrink-0" />
		<span class="truncate">{ttp.name}</span>
	</button>

	{#if requirementDetails.length > 0}
		<Tooltip openDelay={120} closeDelay={80} positioning={{ placement: 'left', gutter: 8 }}>
			<Tooltip.Trigger
				class="mr-1 inline-flex flex-shrink-0 cursor-help items-center gap-0.5 rounded px-1 py-0.5 text-[10px] {prerequisitesFulfilled
					? 'bg-success-100-900 text-success-700-300'
					: 'bg-warning-100-900 text-warning-700-300'}"
				aria-label={`${prerequisitesFulfilled ? 'Prerequisites fulfilled' : 'Prerequisites not fulfilled'}: ${requirementDetails.length} ${requirementDetails.length === 1 ? 'requirement' : 'requirements'}`}
			>
				<Icon
					icon={prerequisitesFulfilled ? 'mdi:check-circle-outline' : 'mdi:alert-circle-outline'}
					width="14"
				/>
				<span class="tabular-nums">{requirementDetails.length}</span>
			</Tooltip.Trigger>
			<Tooltip.Positioner>
				<Tooltip.Content
					class="border-surface-300-700 bg-surface-100-900 z-[90] w-72 rounded border p-2 shadow-xl"
				>
					<div class="mb-1.5 flex items-center gap-1.5 text-xs font-semibold">
						<Icon
							icon={prerequisitesFulfilled ? 'mdi:check-circle' : 'mdi:alert-circle'}
							width="14"
						/>
						{prerequisitesFulfilled ? 'Prerequisites fulfilled' : 'Prerequisites not fulfilled'}
					</div>
					<div class="space-y-1.5">
						{#each requirementDetails as requirement}
							<div class="flex gap-1.5 text-xs">
								<span class="text-surface-700-300 shrink-0 font-medium">{requirement.label}:</span>
								<span class="text-surface-500 min-w-0 break-words">{requirement.value}</span>
							</div>
						{/each}
					</div>
				</Tooltip.Content>
			</Tooltip.Positioner>
		</Tooltip>
	{/if}

	{#if utility !== undefined}
		{#snippet chip()}
			{#if scoreBlocker}
				<Icon icon="mdi:alert-circle" width="14" class="text-error-500" aria-label={blockerLabel} />
			{/if}
			<span class="bg-surface-300-700 h-1.5 w-10 overflow-hidden rounded">
				<span class="block h-full {utilityClass(utility)}" style="width: {pct}%"></span>
			</span>
			<span class="w-6 text-right text-[10px] tabular-nums">{pct}</span>
		{/snippet}

		{#if breakdown && breakdown.length}
			<Tooltip openDelay={120} closeDelay={80} positioning={{ placement: 'right', gutter: 8 }}>
				<Tooltip.Trigger
					class="text-surface-500 hover:text-primary-500 flex flex-shrink-0 cursor-help items-center gap-1 pr-2 pl-1"
					aria-label={scoreBlocker
						? `Utility ${pct} - ${blockerLabel}`
						: `Utility ${pct} - hover for breakdown`}
					title={blockerLabel || 'Hover for utility breakdown'}
				>
					{@render chip()}
				</Tooltip.Trigger>
				<Tooltip.Positioner>
					<Tooltip.Content
						class="bg-surface-100-900 border-surface-300-700 z-[90] w-64 rounded border p-2 shadow-xl"
					>
						<div class="mb-1.5 flex items-center justify-between">
							<span class="text-surface-500 text-[10px] tracking-wide uppercase">Utility</span>
							<span class="text-xs font-semibold">{pct}</span>
						</div>
						<ConsiderationBreakdown {breakdown} />
					</Tooltip.Content>
				</Tooltip.Positioner>
			</Tooltip>
		{:else}
			<span
				class="text-surface-500 flex flex-shrink-0 items-center gap-1 pr-2 pl-1"
				title="Utility {pct}"
			>
				{@render chip()}
			</span>
		{/if}
	{/if}
</div>

<style>
	.card-disabled {
		color: #888;
	}
</style>
