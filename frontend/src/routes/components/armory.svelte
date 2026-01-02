<script lang="ts">
	import type { ArmoryType } from '$lib/model';
	import { onDestroy } from 'svelte';
	import { Switch } from '@skeletonlabs/skeleton-svelte';
	import Icon from '@iconify/svelte';
	import { iconMap } from '$lib/tactic_icons';

	import ActionCard from './action_card.svelte';
	import { Accordion } from '@skeletonlabs/skeleton-svelte';
	import type { TTP } from '$lib/api/index';
	import { getCampaignState, parseArmory } from '$lib/components/CampaignState.svelte';

	const campaign = getCampaignState();

	type ArmoryProps = {
		class?: string;
		targetId: string;
		action: (ttp: TTP) => void;
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
	let openTactic = $state(['Initial Access']);

	$effect(() => { armory = campaign.armory; });

	let applicableTTPs: ArmoryType = $state(new Map());
	$effect(() => {
		campaign.api.GetApplicableTTPs(targetId)
			.then((result: TTP[]) => {
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

	let filteredTtps: TTP[] = $state([]);
	// For Search Input
	let searchTerm: string = $state('');
	// resets language menu if search input is used
	// $: if (searchTerm) selectedLang = '';

	const searchAbilities = () => {
		filteredTtps = Array.from(armory.values())
			.flat()
			.filter((ttp: TTP) => {
				let ttpName = ttp.name.toLowerCase();
				return ttpName.includes(searchTerm.toLowerCase());
			});
	};

	function handleClearWithEscape(event: KeyboardEvent) {
		if (event.key == 'Escape') {
			searchTerm = '';
		}
	}

	function isTTPApplicable(ttp: TTP): boolean {
		// if (showAllTTPs) {
		// 	return true;
		// }
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
		<Switch checked={showAllTTPs} onCheckedChange={(e) => {showAllTTPs = e.checked; }}>
		    <Switch.Control class="preset-filled-secondary-50-950 data-[state=checked]:preset-filled-secondary-500"><Switch.Thumb/></Switch.Control>
			<Switch.Label>Show All</Switch.Label>
			<Switch.HiddenInput />
		</Switch>
		<!-- <label class="flex items-center gap-2">
			Show all
			<Switch
				name="Show All"
				checked={showAllTTPs}
				onCheckedChange={(e) => (showAllTTPs = e.checked)}
			/>
		</label> -->
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
			{#each Array.from(showAllTTPs ? campaign.armory : applicableTTPs) as [tactic, ttps]}
				<hr class="hr" />
				<Accordion.Item
					panelClasses="mb-1 bg-surface-200-800"
					panelPadding="0"
					value={tactic}
					classes="text-surface-contrast-200-800"
					disabled={ttps.length === 0}
				>
    				<Accordion.ItemTrigger class="flex justify-between items-center">
						<Icon icon={iconMap[tactic]} width="24"></Icon>
						<div class="flex w-full items-center">
							<span class="flex-1">{tactic}</span>
							<span class="ml-2 text-xs text-gray-500">
								{applicableTTPs.get(tactic)?.length ?? 0}
							</span>
						</div>
    				</Accordion.ItemTrigger>
				<Accordion.ItemContent class="pl-4 px-0">
						{#each ttps as ttp}
							<ActionCard
								{ttp}
								conditions={ttp.requires}
								icon={iconMap[ttp.tactic]}
								enabled={isTTPApplicable(ttp)}
								onclick={() => sendAction(ttp)}
							/>
						{/each}
					</Accordion.ItemContent>
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
