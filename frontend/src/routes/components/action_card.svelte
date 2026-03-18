<script lang="ts">
	import { AccessLevel } from '$lib/model';
	import Icon from '@iconify/svelte';
	import type { TTP } from '$lib/api';

	interface ActionCardProps {
		ttp: TTP;
		icon: any;
		enabled?: boolean;
		conditions?: Object;
		onclick: (ttp: TTP) => void;
		class?: string | undefined;
	}

	let { ttp, icon, enabled = true, onclick, class: className } : ActionCardProps = $props();

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
		const filteredEntries = Object.entries(ttp.requires).filter(
			([name, value]) => {
				// Exclude kind and accessLevel
				if (name === 'kind' || name === 'accessLevel') return false;
				// Exclude rbacPermissions if it's an empty array
				if (name === 'rbacPermissions' && Array.isArray(value) && value.length === 0) return false;
				return true;
			}
		);
		console.log('Visible requirements for', ttp.name, ':', filteredEntries);
		return filteredEntries.length > 0;
	});

	// export let onClick = (ttp: TTP) => {};
</script>

<button
	onclick={() => onclick(ttp)}
	class={[cardStyle + " hover:bg-surface-400-600 text-xs md:text-sm lg:text-base border-surface-50-950 p-0 pl-4 pt-2 text-left w-full pb-2", className]}
	role="menuitem"
	tabindex="0"
	disabled={!enabled}
	style="overflow: visible;"
>
	<header class="card-header flex items-center gap-2" style="overflow: visible;">
		<Icon icon={displayIcon()} class="inline-block" />
		<span>{ttp.name}</span>
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
								<span class="req-badge badge bg-success-100-900 text-secondary-contrast-200-800 cursor-help text-xs">
									<Icon icon={'carbon-user-admin'} width="12" class="inline-block flex-shrink-0"></Icon>
									<span class="req-text">{formatRbac(perms as { verb?: string; resourceType?: string })}</span>
								</span>
							{/each}
						{/if}
					{:else if name === 'otherFields'}
						{#each Object.entries(value) as [fieldName, val]}
							<span class="req-badge badge bg-surface-100-900 text-secondary-contrast-200-800 cursor-help text-xs">
								<Icon icon={'mdi:dots-horizontal'} width="12" class="inline-block flex-shrink-0"></Icon>
								<span class="req-text">{fieldName}: {JSON.stringify(val)}</span>
							</span>
						{/each}
					{:else}
						<span class="req-badge badge bg-surface-100-900 text-secondary-contrast-200-800 cursor-help text-xs">
							<Icon icon={'mdi:information-outline'} width="12" class="inline-block flex-shrink-0"></Icon>
							<span class="req-text">{name}: {JSON.stringify(value)}</span>
						</span>
					{/if}
				{/if}
			{/each}
		{/if}
	</header>
</button>

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
		width: 22px;
		overflow: visible;
		transition: width 0.3s ease-in-out;
		z-index: 10;
		padding: 0.125rem 0.25rem;
	}

	.req-badge:hover {
		width: auto;
		min-width: 22px;
	}

	.req-text {
		opacity: 0;
		max-width: 0;
		overflow: hidden;
		transition: opacity 0.2s ease-in-out, max-width 0.3s ease-in-out;
	}

	.req-badge:hover .req-text {
		opacity: 1;
		max-width: 200px;
	}
</style>
