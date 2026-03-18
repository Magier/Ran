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
>
	<header class="card-header">
		<Icon icon={displayIcon()} class="inline-block" />
		<span>{ttp.name}</span>
	</header>
	<!-- <section class="p-4" /> -->
	{#if hasVisibleRequirements()}
		<footer class="card-footer flex flex-wrap gap-2 py-2">
			{#each Object.entries(ttp.requires) as [name, value]}
				{#if !!value}
					{#if name === 'kind'}
						<!-- <span class="badge bg-tertiary-100-900 text-tertiary-contrast-200-800">
							<Icon icon={'carbon-hexagon-outline'} width="16"></Icon>
							{value}
						</span> -->
					 {:else if name === 'accessLevel'}
						<!-- <span class="badge bg-success-100-900 text-secondary-contrast-200-800">
							<Icon icon={'carbon-user-admin'} width="16"></Icon>
							{value}
						</span> -->
					{:else if name === 'rbacPermissions'}
						{#each Array.from(value ?? []) as perms}
							<span class="badge bg-success-100-900 text-secondary-contrast-200-800">
								<Icon icon={'carbon-user-admin'} width="16"></Icon>
								{formatRbac(perms as { verb?: string; resourceType?: string })}
							</span>
						{/each}
					{:else if name === 'otherFields'}
						{#each Object.entries(value) as [name, val]}
							<span class="chip variant-filled-surface mr-1 max-w-full truncate">
								{name}: {JSON.stringify(val)}
							</span>
						{/each}
					{:else}
						<!-- adjust chip style if the condition is fullfilled or not -->
						<span class="badge bg-surface-100-900 text-secondary-contrast-200-800">
							<!-- <span class="chip variant-filled-surface mr-1 max-w-full truncate"> -->
							{name}: {JSON.stringify(value)}
						</span>
					{/if}
				{/if}
			{/each}
		</footer>
	{/if}
</button>

<style>
	.card-disabled {
		color: #888;
	}
</style>
