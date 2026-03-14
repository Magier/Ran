<script lang="ts">
	import type { ArmoryType } from '$lib/model';
	import { onDestroy } from 'svelte';
	import { Switch } from '@skeletonlabs/skeleton-svelte';
	import Icon from '@iconify/svelte';
	import { iconMap } from '$lib/tactic_icons';

	import ActionCard from './action_card.svelte';
	import { Accordion } from '@skeletonlabs/skeleton-svelte';
	import type { TTP, Node } from '$lib/api/index';
	import { getCampaignState, parseArmory } from '$lib/components/CampaignState.svelte';

	const campaign = getCampaignState();

	type ArmoryProps = {
		class?: string;
		targetId: string;
		target?: Node;
		action: (ttp: TTP) => void;
		focusSearch?: () => void;
	};

	let { class: className = '', targetId, target, action: sendAction, focusSearch = $bindable(() => {}) }: ArmoryProps = $props();

	let searchInputElement: HTMLInputElement | undefined = $state();

	// $: selectedConditions = { ...globalConditions, ...(selectedNode ?? {}) };
	let armory: ArmoryType = $state(new Map());
	let showAllTTPs: boolean = $state(false);
	let applicableTTPs: ArmoryType = $state(new Map());
	let filteredTtps: TTP[] = $state([]);
	let searchTerm: string = $state('');
	let openTactic = $state(['Initial Access']);
	let isShiftPressed: boolean = $state(false);

	$effect(() => { armory = campaign.armory; });

	let shownTTPs: ArmoryType = $derived.by(() => {
		if (searchTerm) {
			return filteredTtps;
		} else if (showAllTTPs) {
			return Array.from(armory.entries());
		} else {
			return Array.from(applicableTTPs.entries());
		}
	});
 
	// Fetch applicable TTPs whenever the target node changes or its state updates
	$effect(() => {
		// Track targetId and node state properties that affect applicable TTPs
		const nodeState = target ? {
			compromised: target.compromised,
			accessLevel: target.accessLevel,
			entity: target.entity
		} : null;
		
		if (!targetId) {
			applicableTTPs = new Map();
			return;
		}
		
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

	const searchAbilities = () => {
		const src = showAllTTPs ? armory : applicableTTPs;

		filteredTtps = Array.from(src.entries())
			.filter(([tactic, ttps]) => 
				ttps.some((ttp: TTP) => 
					ttp.name.toLowerCase().includes(searchTerm.toLowerCase())
				)
			)
			.map(([tactic, ttps]) => [
				tactic,
				ttps.filter((ttp: TTP) => 
					ttp.name.toLowerCase().includes(searchTerm.toLowerCase())
				)
			]);
		console.log("filteredTtps", filteredTtps);
	};

	$effect(() => {
		focusSearch = () => {
			searchInputElement?.focus();
		};
	});


	function handleClearWithEscape(event: KeyboardEvent) {
		if (event.key == 'Escape') {
			searchTerm = '';
			searchInputElement?.blur();
		}
	}

	function handleKeyDown(event: KeyboardEvent) {
		if (event.key === 'Shift') {
			isShiftPressed = true;
		}
	}

	function handleKeyUp(event: KeyboardEvent) {
		if (event.key === 'Shift') {
			isShiftPressed = false;
		}
	}

	$effect(() => {
		window.addEventListener('keydown', handleKeyDown);
		window.addEventListener('keyup', handleKeyUp);

		return () => {
			window.removeEventListener('keydown', handleKeyDown);
			window.removeEventListener('keyup', handleKeyUp);
		};
	});

	function isTTPApplicable(ttp: TTP): boolean {
		let procedures = applicableTTPs.get(ttp.tactic) || [];
		for (let proc of procedures) {
			if (proc.name === ttp.name) {
				return true;
			}
		}
		return false;
	}

	function onActionSelected(ttp: TTP) {
		searchTerm = ''; // reset the filter again to show all TTPs 
		sendAction(ttp);
	}
</script>

<div class="bg-surface-100-900 inset-y-0 right-0 flex flex-col {className}">
	<!-- Fixed header section -->
	<div class="flex-shrink-0">
		<div class="my-2 flex items-center justify-between">
			<span class="px-2 text-xl">Armory</span>
			<Switch checked={showAllTTPs} onCheckedChange={(e) => {showAllTTPs = e.checked; }}>
				<Switch.Control class=""><Switch.Thumb/></Switch.Control>
				<Switch.Label class="mr-2">Show All</Switch.Label>
				<Switch.HiddenInput />
			</Switch>
		</div>
		<div class="mx-4 mb-2">
			<input
				id="search-box"
				type="search"
				placeholder="Search... (Press 'a')"
				class="input rounded-container-token"
				bind:this={searchInputElement}
				bind:value={searchTerm}
				onkeydown={handleClearWithEscape}
				oninput={searchAbilities}
			/>
		</div>
	</div>

	<!-- Scrollable content section -->
	<div class="flex-1 overflow-y-auto min-h-0">
		{#if shownTTPs.length === 0}
			<div class="flex h-full items-center justify-center text-center text-gray-500">
				No TTPs available. <br>
				Please select an entity in the graph.
			</div>
		{:else}
			<Accordion value={openTactic} onValueChange={(e) => (openTactic = e.value)} collapsible>
				{#each Array.from(shownTTPs) as [tactic, ttps]}
					<hr class="hr" />
					<Accordion.Item
						value={tactic}
						class="text-surface-contrast-200-800"
						disabled={ttps?.length === 0}
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
						<Accordion.ItemContent class="px-0 py-0 mb-1 bg-surface-200-800">
							{#each ttps as ttp}
								<ActionCard
									{ttp}
									conditions={ttp.requires}
									icon={iconMap[ttp.tactic]}
									enabled={isShiftPressed || isTTPApplicable(ttp)}
									onclick={() => onActionSelected(ttp)}
								/>
								<hr class="hr h-1 bg-surface-300-700" />
							{/each}
						</Accordion.ItemContent>
					</Accordion.Item>
				{/each}
			</Accordion>
		{/if}
	</div>
</div>
