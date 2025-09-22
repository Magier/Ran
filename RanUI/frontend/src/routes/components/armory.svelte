<script lang="ts">
	import type { ArmoryType } from '$lib/model';
	import { onDestroy, onMount } from 'svelte';
	import { Switch } from '@skeletonlabs/skeleton-svelte';
	import store, { parseArmory } from '$lib/stores/store';
	import Icon from '@iconify/svelte';

	import ActionCard from './action_card.svelte';
	import { Accordion } from '@skeletonlabs/skeleton-svelte';
	import { GetApplicableTTPs  } from '$lib/wailsjs/go/main/App';
	import { domain } from '$lib/wailsjs/go/models';
	import { getCampaignState } from '$lib/components/CampaignState.svelte';

	const campaignState = getCampaignState();

	type ArmoryProps = {
		class?: string;
		targetId: string;
		action: (ttp: domain.TTP) => void;
	};
	// import { filteredArmory, searchAbilities } from '$lib/stores/armoryStore.js';
	const iconMap: Record<string, string> = {
		'Resource Development': 'healthicons:entry-outline',
		'Initial Access': 'material-symbols:door-open-outline',
		Discovery: 'material-symbols:schema-outline',
		Execution: 'material-symbols:settings-slow-motion',
		Persistence: 'game-icons:life-jacket',
		'Privilege Escalation': 'mdi:account-arrow-up',
		'Defense Evasion': 'game-icons:hood',
		'Credential Access': 'mdi:key-chain-variant',
		'Command And Control': 'material-symbols:satellite-alt',
		Collection: 'game-icons:receive-money',
		'Lateral Movement': 'material-symbols:timeline',
		Impact: 'game-icons:falling-bomb',
		Other: 'game-icons:dig-dug'
	};

	// ==============================================================

	// TODO: implement filtered armory, so it can work with selectedNode
	// -maybe: store the search term in the store?
	// 				similar to: https://www.youtube.com/watch?v=lrzHaTcpRh8
	// ==============================================================

	let { class: className = '', targetId, action: sendAction }: ArmoryProps = $props();

	// export let selectedNode: Object | null = null;
	// export let globalConditions: Object = {};
	// $: selectedConditions = { ...globalConditions, ...(selectedNode ?? {}) };
	let armory: ArmoryType = $state(new Map());
	let showAllTTPs: boolean = $state(false);
	let openTactic = $state(['InitialAccess']);

	$effect(() => { armory = campaignState.armory; });

	let applicableTTPs: ArmoryType = $state(new Map());
	$effect(() => {
		GetApplicableTTPs(targetId)
			.then((result: domain.TTP[]) => {
				applicableTTPs = parseArmory(result);

				// if there is only tactic, open it by default
				if (applicableTTPs.size === 1) {
					openTactic = [Array.from(applicableTTPs.keys())[0]];
				}
			})
			.catch((err) => {
				console.error('Error fetching applicable TTPs:', err);
			});
	});

	// const filteredArmoryStore = createSearchStore(armory);
	// const unsubscribe = filteredArmory.subscribe((ttp) => searchAbilities(ttp));

	// function searchAbilities = () => {
	// 	return filteredAbilities =
	// };

	onDestroy(() => {
		// unsubscribe();
	});

	let filteredTtps: domain.TTP[] = $state([]);
	// For Search Input
	let searchTerm: string = $state('');
	// resets language menu if search input is used
	// $: if (searchTerm) selectedLang = '';

	const searchAbilities = () => {
		filteredTtps = Array.from(armory.values())
			.flat()
			.filter((ttp: domain.TTP) => {
				let ttpName = ttp.name.toLowerCase();
				return ttpName.includes(searchTerm.toLowerCase());
			});
	};

	function handleClearWithEscape(event: KeyboardEvent) {
		if (event.key == 'Escape') {
			searchTerm = '';
		}
	}

	function isTTPApplicable(ttp: domain.TTP): boolean {
		if (showAllTTPs) {
			return true;
		}
		let procedures = applicableTTPs.get(ttp.tactic) || [];
		for (let proc of procedures) {
			if (proc.name === ttp.name) {
				return true;
			}
		}
		return false;
	}
</script>

<div class="bg-surface-100-900 inset-y-0 right-0 {className}">
	<div class="my-2 flex items-center justify-between">
		<span class="px-2 text-xl">Armory</span>
		<label class="flex items-center gap-2">
			Show all
			<Switch
				name="Show All"
				checked={showAllTTPs}
				onCheckedChange={(e) => (showAllTTPs = e.checked)}
			/>
		</label>
	</div>
	<!-- <div class="mx-4 mb-2">
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
	</div> -->
	<div>
		<Accordion value={openTactic} onValueChange={(e) => (openTactic = e.value)} collapsible>
			{#each Array.from(showAllTTPs ? campaignState.armory : applicableTTPs) as [tactic, ttps]}
				<hr class="hr" />
				<Accordion.Item
					panelClasses="px-2 mb-1"
					panelPadding="0"
					value={tactic}
					classes="text-surface-contrast-200-800"
					disabled={ttps.length === 0}
				>
					{#snippet lead()}
						<Icon icon={iconMap[tactic]} width="24"></Icon>
					{/snippet}
					{#snippet control()}
						<div class="flex w-full items-center">
							<span class="flex-1">{tactic}</span>
							<span class="ml-2 text-xs text-gray-500"
								>{applicableTTPs.get(tactic)?.length ?? 0}</span
							>
						</div>
					{/snippet}
					{#snippet panel()}
						{#each ttps as ttp}
							<ActionCard
								{ttp}
								conditions={ttp.requires}
								icon={iconMap[ttp.tactic]}
								enabled={isTTPApplicable(ttp)}
								onclick={() => sendAction(ttp)}
							/>
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
