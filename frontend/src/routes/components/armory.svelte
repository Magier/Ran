<script lang="ts">
	import type { ArmoryType } from '$lib/model';
	import { onDestroy } from 'svelte';
	import Icon from '@iconify/svelte';
	import { iconMap } from '$lib/tactic_icons';

	import ActionCard from './action_card.svelte';
	import Recommendations from './recommendations.svelte';
	import ScoringTuner from './scoring_tuner.svelte';
	import { Accordion, Tabs } from '@skeletonlabs/skeleton-svelte';
	import type { TTP, Node, ScoredCandidate } from '$lib/api/index';
	import { getCampaignState, parseArmory } from '$lib/components/CampaignState.svelte';

	const campaign = getCampaignState();

	type ArmoryProps = {
		class?: string;
		targetId: string;
		target?: Node;
		action: (ttp: TTP) => void;
		/** Execute a recommendation against its own target (selects the target first). */
		runRecommendation: (rec: ScoredCandidate) => void;
		focusSearch?: () => void;
	};

	let { class: className = '', targetId, target, action: sendAction, runRecommendation, focusSearch = $bindable(() => {}) }: ArmoryProps = $props();

	let activeTab: string = $state('actions');

	let searchInputElement: HTMLInputElement | undefined = $state();

	// $: selectedConditions = { ...globalConditions, ...(selectedNode ?? {}) };
	let armory: ArmoryType = $state(new Map());
	let showAllTTPs: boolean = $state(false);
	let applicableTTPs: ArmoryType = $state(new Map());
	let filteredTtps: TTP[] = $state([]);
	let searchTerm: string = $state('');
	let openTactic = $state(['Initial Access']);
	let isShiftPressed: boolean = $state(false);
	// Scored candidate per applicable TTP for the selected target (ttp_id → candidate).
	// Holds the full breakdown so each action card can explain its score on hover.
	let scoredByTtp: Map<string, ScoredCandidate> = $state(new Map());

	$effect(() => { armory = campaign.armory; });

	// Score the applicable actions for the selected target. The armory already
	// prefilters to applicable TTPs, so we only score those — utility per action
	// against this one target, rather than every action × every target.
	$effect(() => {
		// Re-score whenever the target, campaign facts, or scoring profile change.
		void campaign.graph;
		void campaign.entities;
		void campaign.scoringVersion;

		if (!targetId) {
			scoredByTtp = new Map();
			return;
		}

		campaign.api
			.GetRecommendations(targetId)
			.then((recs: ScoredCandidate[]) => {
				scoredByTtp = new Map(recs.map((r) => [r.ttp_id, r]));
			})
			.catch((err: unknown) => {
				console.error('Error scoring applicable TTPs:', err);
				scoredByTtp = new Map();
			});
	});

	// Order TTPs within a tactic by utility (desc); unscored actions sink to the bottom.
	function byUtility(ttps: TTP[]): TTP[] {
		return [...ttps].sort(
			(a, b) => (scoredByTtp.get(b.id)?.utility ?? -1) - (scoredByTtp.get(a.id)?.utility ?? -1)
		);
	}

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
	<Tabs value={activeTab} onValueChange={(e) => (activeTab = e.value)} class="flex-1 min-h-0 flex flex-col">
		<Tabs.List class="grid h-10 w-full flex-none grid-cols-2 items-stretch gap-0 overflow-hidden border-b border-surface-300-700 p-0">
			<Tabs.Trigger
				value="actions"
				class="m-0 flex h-full min-h-0 w-full self-stretch items-center justify-center rounded-none p-0 text-center transition-colors {activeTab === 'actions' ? 'bg-surface-200-800 text-primary-700-300 shadow-[inset_0_-2px_0_var(--color-primary-500)] font-semibold' : 'text-surface-500 hover:bg-surface-200-800/60'}"
			>Actions</Tabs.Trigger>
			<Tabs.Trigger
				value="recommendations"
				class="m-0 flex h-full min-h-0 w-full self-stretch items-center justify-center rounded-none p-0 text-center transition-colors {activeTab === 'recommendations' ? 'bg-surface-200-800 text-primary-700-300 shadow-[inset_0_-2px_0_var(--color-primary-500)] font-semibold' : 'text-surface-500 hover:bg-surface-200-800/60'}"
			>
				<span class="flex items-center justify-center gap-1">
					<Icon icon="mdi:lightbulb-on-outline" width="16" class="text-warning-500" />
					Recommendations
				</span>
			</Tabs.Trigger>
		</Tabs.List>

		<!-- Actions: all applicable TTPs grouped by tactic, scored per target -->
		<Tabs.Content value="actions" class="flex-1 min-h-0 flex flex-col !p-0 !m-0">
			<!-- Toolbar: search + scope toggle on one compact row -->
			<div class="flex-shrink-0 flex items-center gap-2 px-2 py-2 border-b border-surface-200-800">
				<input
					id="search-box"
					type="search"
					placeholder="Search… (a)"
					class="input rounded-container-token flex-1 min-w-0 !py-1 text-sm"
					bind:this={searchInputElement}
					bind:value={searchTerm}
					onkeydown={handleClearWithEscape}
					oninput={searchAbilities}
				/>
				<div
					class="flex shrink-0 rounded-md overflow-hidden border border-surface-300-700 text-xs"
					role="group"
					aria-label="Action scope"
				>
					<button
						class="px-2 py-1 transition-colors {showAllTTPs
							? 'text-surface-500 hover:bg-surface-200-800'
							: 'bg-primary-500 text-primary-contrast-500'}"
						aria-pressed={!showAllTTPs}
						onclick={() => (showAllTTPs = false)}
						title="Show only actions applicable to the selected target"
					>
						Applicable
					</button>
					<button
						class="px-2 py-1 border-l border-surface-300-700 transition-colors {showAllTTPs
							? 'bg-primary-500 text-primary-contrast-500'
							: 'text-surface-500 hover:bg-surface-200-800'}"
						aria-pressed={showAllTTPs}
						onclick={() => (showAllTTPs = true)}
						title="Show every action, including ones not yet applicable"
					>
						All
					</button>
				</div>
			</div>

			<div class="flex-1 overflow-y-auto min-h-0">
				{#if shownTTPs.length === 0}
					<div class="flex h-full items-center justify-center text-center text-gray-500">
						No TTPs available. <br>
						Please select an entity in the graph.
					</div>
				{:else}
					<Accordion value={openTactic} onValueChange={(e) => (openTactic = e.value)} collapsible class="!gap-0 !space-y-0 bg-surface-200-800">
						{#each Array.from(shownTTPs) as [tactic, ttps]}
							<Accordion.Item
								value={tactic}
								class="text-surface-contrast-200-800 !p-0 mb-0 !gap-0"
								disabled={ttps?.length === 0}
							>
								<Accordion.ItemTrigger class="flex justify-between items-center bg-surface-200-800 hover:bg-surface-300-700 hover:text-primary-800-200 text-m lg:text-l border-t border-surface-300-700 p-3 !m-0">
									<Icon icon={iconMap[tactic]} width="26" class="flex-shrink-0"></Icon>
									<div class="flex w-full items-center ml-2">
										<span class="flex-1">{tactic}</span>
										<span class="ml-2 px-2 py-0.5 rounded text-xs bg-surface-200-800 text-surface-contrast-200-800">
											{applicableTTPs.get(tactic)?.length ?? 0}
										</span>
									</div>
								</Accordion.ItemTrigger>
								<Accordion.ItemContent class="!p-0 !m-0 !gap-0 bg-surface-100-900">
									{#each byUtility(ttps) as ttp}
										<div class="ml-3 border-t-1 border-surface-400-600 bg-surface-200-800 hover:text-primary-800-200">
											<ActionCard
												{ttp}
												conditions={ttp.requires}
												icon={iconMap[ttp.tactic]}
												enabled={isShiftPressed || isTTPApplicable(ttp)}
												utility={scoredByTtp.get(ttp.id)?.utility}
												breakdown={scoredByTtp.get(ttp.id)?.breakdown}
												onclick={() => onActionSelected(ttp)}
											/>
										</div>
									{/each}
								</Accordion.ItemContent>
							</Accordion.Item>
						{/each}
					</Accordion>
				{/if}
			</div>
		</Tabs.Content>

		<!-- Recommendations: utility-scored next steps across all targets -->
		<Tabs.Content value="recommendations" class="flex-1 min-h-0 flex flex-col !p-0 !m-0">
			<Recommendations run={runRecommendation}>
				{#snippet actions()}
					<ScoringTuner />
				{/snippet}
			</Recommendations>
		</Tabs.Content>
	</Tabs>
</div>
