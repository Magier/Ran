<script lang="ts">
	import type { TTP } from '$lib/model';
	import { onDestroy, onMount } from 'svelte';
	import store from '$lib/stores/store';
	import Icon from '@iconify/svelte';

	type ArmoryProps = {
		class?: string;
		action: (ttp: TTP) => void;
	};
	// import { filteredArmory, searchAbilities } from '$lib/stores/armoryStore.js';

	import ActionCard from './action_card.svelte';
	import { Accordion } from '@skeletonlabs/skeleton-svelte';

	const iconMap: Record<string, string> = {
		'Resource Development': 'healthicons:entry-outline',
		'Initial Access': 'material-symbols:door-open-outline',
		Execution: 'material-symbols:settings-slow-motion',
		Persistence: 'game-icons:life-jacket',
		'Privilege Escalation': 'mdi:account-arrow-up',
		'Defense Evasion': 'game-icons:hood',
		'Credential Access': 'mdi:key-chain-variant',
		'Command And Control': 'material-symbols:satellite-alt',
		Discovery: 'material-symbols:schema-outline',
		'Lateral Movement': 'material-symbols:timeline',
		Impact: 'game-icons:falling-bomb',
		Other: 'game-icons:dig-dug'
	};

	// ==============================================================

	// TODO: implement filtered armory, so it can work with selectedNode
	// -maybe: store the search term in the store?
	// 				similar to: https://www.youtube.com/watch?v=lrzHaTcpRh8
	// ==============================================================

	let { class: className = '', action: sendAction }: ArmoryProps = $props();

	// export let selectedNode: Object | null = null;
	// export let globalConditions: Object = {};
	// $: selectedConditions = { ...globalConditions, ...(selectedNode ?? {}) };
	let armory: Map<string, TTP[]> = $state(new Map());

	// const filteredArmoryStore = createSearchStore(armory);
	// const unsubscribe = filteredArmory.subscribe((ttp) => searchAbilities(ttp));

	// function searchAbilities = () => {
	// 	return filteredAbilities =
	// };

	onMount(() => {
		store.armory((new_armory: Map<string, TTP[]>) => {
			armory = new_armory;
			console.log(armory);
		});
		store.sendMessage('get-armory', {});
	});

	onDestroy(() => {
		// unsubscribe();
	});

	let filteredTtps: TTP[] = $state([]);
	// For Search Input
	let searchTerm: string = $state('');
	// resets language menu if search input is used
	// $: if (searchTerm) selectedLang = '';

	const searchAbilities = () => {
		filteredTtps = armory.filter((ttp) => {
			let ttpName = ttp.name.toLowerCase();
			return ttpName.includes(searchTerm.toLowerCase());
		});
	};

	function handleClearWithEscape(event: KeyboardEvent) {
		if (event.key == 'Escape') {
			searchTerm = '';
		}
	}
	let tag = $state('IconInitialAccess');
</script>

<div class="bg-surface-100-800-token inset-y-0 right-0 h-full w-80 {className}">
	<h1>Armory</h1>
	<div class="mx-4 mb-2">
		<label for="search-box">Search/Filter</label>
		<input
			id="search-box"
			type="search"
			placeholder="Search..."
			class="input rounded-container-token"
			bind:value={searchTerm}
			onkeydown={handleClearWithEscape}
			oninput={searchAbilities}
		/>
	</div>
	<div>
		<Accordion>
			{#each Array.from(armory) as [tactic, ttps]}
				<hr class="hr" />
				<Accordion.Item value={tactic} disabled={ttps.length === 0}>
					{#snippet lead()}
						<Icon icon={iconMap[tactic]} width="24"></Icon>
					{/snippet}
					{#snippet control()}
						{tactic}
					{/snippet}
					{#snippet panel()}
						{#each ttps as ttp}
							<ActionCard {ttp} icon={iconMap[ttp.tactic]} onclick={() => sendAction(ttp)} />
						{/each}
					{/snippet}
				</Accordion.Item>
			{/each}
		</Accordion>

		<!-- <TreeView open class=" space-y-2 overflow-y-auto overflow-x-hidden">

		{#if searchTerm && filteredTtps.length === 0}
			Nothing to see :(
		{:else if filteredTtps.length > 0}
			{#each filteredTtps as ttp}
				{#each ttps as ttp}
					<ActionCard {ttp} icon={iconMap[ttp.tactic]} onclick={() => sendAction(ttp)} />
				{/each}
			{:else}
				{#each Object.entries(armory) as [id, ttp]}
					<ActionCard
						{ttp}
						icon={iconMap[ttp.tactic]}
						conditions={selectedConditions}
						onclick={() => sendAction(ttp)}
					/>
				{/each}
				 <TreeViewItem disabled={ttps.length === 0}>
					<svelte:fragment slot="lead">
						<svelte:component this={iconMap[tactic]} />
					</svelte:fragment>
					{tactic}
					<svelte:fragment slot="children">
						{#each ttps as ttp}
							<ActionCard
								{ttp}
								icon={iconMap[tactic]}
								onClick={sendAction}
								on:click={() => sendAction(ttp)}
							/>
						{/each}
					</svelte:fragment>
				</TreeViewItem> -->
		<!-- {/each} -->
		<!-- {#each Object.entries($filteredArmory.filtered) as [tactic, ttps]}
				<TreeViewItem disabled={ttps.length === 0}>
					<svelte:fragment slot="lead">
						<svelte:component this={iconMap[tactic]} />
					</svelte:fragment>
					{tactic}
					<svelte:fragment slot="children">
						{#each ttps as ttp}
							<ActionCard
								{ttp}
								icon={iconMap[tactic]}
								onClick={sendAction}
								on:click={() => sendAction(ttp)}
							/>
						{/each}
					</svelte:fragment>
				</TreeViewItem> 
			{/each}
		{/if}
		<!-- </TreeView>  -->
	</div>
</div>
