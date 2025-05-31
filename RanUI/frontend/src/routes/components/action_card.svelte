<script lang="ts">
	import { AccessLevel } from '$lib/model';
	import { domain } from '$lib/wailsjs/go/models';
	import Icon from '@iconify/svelte';

	interface ActionCardProps {
		ttp: domain.TTP;
		icon: any;
		enabled?: boolean;
		conditions?: Object;
		onclick: (ttp: domain.TTP) => void;
	}

	let { ttp, icon, enabled = true, onclick }: ActionCardProps = $props();

	let cardStyle = $derived(
		enabled ? 'card-hover bg-surface-200-800-token' : 'card-disabled bg-surface-50-900-token'
	);

	function checkConditions(ttp: domain.TTP, conditions: Object) {
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
				} else if (attr === 'can') {
					let has_capability = false;
					for (let cap of conditions['can']) {
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

	// export let onClick = (ttp: domain.TTP) => {};
</script>

<button
	onclick={() => onclick(ttp)}
	class="card {cardStyle} w-full p-1 text-left"
	role="menuitem"
	tabindex="0"
	disabled={!enabled}
>
	<header class="card-header">
		<Icon {icon} class="inline-block" />
		<span>{ttp.name}</span>
	</header>
	<!-- <section class="p-4" /> -->
	{#if ttp.requires && Object.keys(ttp.requires).length > 0}
		<footer class="card-footer py-2">
			{#each Object.entries(ttp.requires) as [name, value]}
				{#if !!value}
					{#if name === 'AccessLevel'}
						{#if Object.keys(value).length > 0}
							<span class="chip variant-filled-surface mr-1">
								{name}: {Object.keys(value).join(' or ')}
							</span>
						{/if}
					{:else if name === 'OtherFields'}
						{#each Object.entries(value) as [name, val]}
							<span class="chip variant-filled-surface mr-1">
								{name}: {JSON.stringify(val)}
							</span>
						{/each}
					{:else}
						<!-- adjust chip style if the condition is fullfilled or not -->
						<span class="chip variant-filled-surface mr-1">
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
