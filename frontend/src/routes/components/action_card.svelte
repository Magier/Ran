<script lang="ts">
	import type { TTP } from '../model';

	export let ttp: TTP;
	export let icon;
	export let conditions: Object = {};
	$: requirementsSatisfied = checkConditions(ttp, conditions);
	$: cardStyle = requirementsSatisfied
		? 'card-hover bg-surface-200-700-token'
		: 'card-disabled bg-surface-50-900-token';

	function checkConditions(ttp: TTP, conditions: Object) {
		// no requirements mean the action is always possible
		if (Object.keys(ttp.requires).length == 0) return true;
		if (conditions) {
			for (let [attr, value] of Object.entries(ttp.requires)) {
				if (!conditions.hasOwnProperty(attr)) return false;
				if (attr === 'access_level') {
					if (conditions[attr] < value) return false;
				} else if (attr === 'can') {
					let has_capability = false;
					for (let cap of conditions['can']) {
						if (cap == value) {
							has_capability = true;
							break;
						}
					}
					if (!has_capability) return false;
				} else if (conditions[attr] !== value) {
					return false;
				}
			}
		}

		return true;
	}

	export let onClick = (ttp: TTP) => {};
</script>

<button
	on:click={() => onClick(ttp)}
	class="card {cardStyle} p-1 text-left w-full"
	role="menuitem"
	tabindex="0"
	disabled={!requirementsSatisfied}
>
	<header class="card-header">
		<svelte:component this={icon} class="inline-block" />
		<span class="h5">{ttp.name}</span>
	</header>
	<!-- <section class="p-4" /> -->
	{#if ttp.requires && Object.keys(ttp.requires).length > 0}
		<footer class="card-footer py-2">
			{#each Object.entries(ttp.requires) as [name, value]}
				<!-- adjust chip style if the condition is fullfilled or not -->
				<span class="chip variant-filled-surface mr-1">
					{name}: {value}
				</span>
			{/each}
		</footer>
	{/if}
</button>

<style>
	.card-disabled {
		color: #888;
	}
</style>
