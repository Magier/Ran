<script lang="ts">
	import type { TTP } from '$lib/model';
	import { AccessLevel } from '$lib/model';
	import { IsActionSatisfied } from '$lib/wailsjs/go/main/App';
	import Icon from '@iconify/svelte';

	interface ActionCardProps {
		ttp: TTP;
		targetId?: string;
		icon: any;
		conditions?: Object;
		onclick: (ttp: TTP) => void;
	}

	let { ttp, targetId, icon, conditions, onclick }: ActionCardProps = $props();

	let requirementsSatisfied = $state(false);
	$effect(() => {
		IsActionSatisfied(ttp.id, targetId!)
			.then((result: boolean) => {
				console.log(`Requirements ${ttp.name} Satisfied: `, result);
				requirementsSatisfied = result;
			})
			.catch((err) => {
				console.error(`Error checking requirements for ${ttp.name}: `, err);
			});
	});

	let cardStyle = $derived(
		requirementsSatisfied
			? 'card-hover bg-surface-200-700-token'
			: 'card-disabled bg-surface-50-900-token'
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

	// export let onClick = (ttp: TTP) => {};
</script>

<button
	onclick={() => onclick(ttp)}
	class="card {cardStyle} w-full p-1 text-left"
	role="menuitem"
	tabindex="0"
	disabled={!requirementsSatisfied}
>
	<header class="card-header">
		<Icon {icon} class="inline-block" />
		<span>{ttp.name}</span>
	</header>
	<!-- <section class="p-4" /> -->
	{#if ttp.requires && Object.keys(ttp.requires).length > 0}
		<footer class="card-footer py-2">
			{#each Object.entries(ttp.requires) as [name, value]}
				{#if value}
					{#if name === 'AccessLevel'}
						{#if Object.keys(value).length > 0}
							<span class="chip variant-filled-surface mr-1">
								{name}: {Object.keys(value).join(' or ')}
							</span>
						{/if}
					{:else}
						<!-- adjust chip style if the condition is fullfilled or not -->
						<span class="chip variant-filled-surface mr-1">
							{name}: {value}
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
