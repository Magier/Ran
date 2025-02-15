<script lang="ts">
	import { createEventDispatcher, onDestroy, onMount } from 'svelte';
	import IconInitialAccess from '~icons/healthicons/entry-outline';
	import IconPrivilegeEscalation from '~icons/mdi/account-arrow-up';
	import IconCredentialAccess from '~icons/mdi/key-chain-variant';
	import IconDiscovery from '~icons/mdi/gold';
	import IconExecution from '~icons/material-symbols/settings-slow-motion';
	import store from '$lib/stores/store';
	// import { filteredArmory, searchAbilities } from '$lib/stores/armoryStore.js';

	import {
		TreeView
		// TreeViewItem,
		// RecursiveTreeView,
		// type TreeViewNode
	} from '@skeletonlabs/skeleton';
	import type { TTP } from '../model.js';
	import ActionCard from './action_card.svelte';

	const iconMap = {
		InitialAccess: IconInitialAccess,
		Execution: IconExecution,
		Persistence: IconExecution,
		PrivilegeEscalation: IconPrivilegeEscalation,
		DefenseEvasion: IconPrivilegeEscalation,
		CredentialAccess: IconCredentialAccess,
		Discovery: IconDiscovery,
		LateralMovement: IconDiscovery,
		Impact: IconDiscovery
	};

	// ==============================================================

	// TODO: implement filtered armory, so it can work with selectedNode
	// -maybe: store the search term in the store?
	// 				similar to: https://www.youtube.com/watch?v=lrzHaTcpRh8
	// ==============================================================

	const dispatch = createEventDispatcher();

	let className = '';
	export { className as class };

	export let selectedNode: Object | null = null;
	export let globalConditions: Object = {};
	$: selectedConditions = { ...globalConditions, ...(selectedNode ?? {}) };
	let armory: TTP[] = [];
	// let armory: Map<string, TTP[]> = {};

	// const filteredArmoryStore = createSearchStore(armory);
	// const unsubscribe = filteredArmory.subscribe((ttp) => searchAbilities(ttp));

	function sendAction(ttp: TTP) {
		dispatch('action', { ...ttp });
	}

	// function searchAbilities = () => {
	// 	return filteredAbilities =
	// };

	onMount(() => {
		store.armory((new_armory: Map<string, TTP[]>) => {
			armory = new_armory;
		});
		store.sendMessage('armory', {});
	});

	onDestroy(() => {
		// unsubscribe();
	});

	let filteredTtps: TTP[] = [];
	// For Search Input
	let searchTerm: string = '';
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
</script>

<div class="h-full w-80 inset-y-0 right-0 bg-surface-100-800-token {className}">
	<div class="mx-4 mb-2">
		<h1>Search/Filter</h1>
		<input
			type="search"
			placeholder="Search..."
			class="input rounded-container-token"
			bind:value={searchTerm}
			on:keydown|stopPropagation={handleClearWithEscape}
			on:input={searchAbilities}
		/>
	</div>
	<TreeView open class=" overflow-y-auto overflow-x-hidden space-y-2">
		{#if searchTerm && filteredTtps.length === 0}
			Nothing to see :(
		{:else if filteredTtps.length > 0}
			{#each filteredTtps as ttp}
				<!-- {#each ttps as ttp} -->
				<ActionCard
					{ttp}
					icon={iconMap[ttp.tactics[0]]}
					onClick={sendAction}
					on:click={() => sendAction(ttp)}
				/>
			{/each}
		{:else}
			{#each Object.entries(armory) as [id, ttp]}
				<ActionCard
					{ttp}
					icon={iconMap[ttp.tactics[0]]}
					conditions={selectedConditions}
					onClick={sendAction}
					on:click={() => sendAction(ttp)}
				/>

				<!-- {/each} -->
				<!-- <TreeViewItem disabled={ttps.length === 0}>
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
				</TreeViewItem> -->
			{/each}
		{/if}
	</TreeView>
</div>
