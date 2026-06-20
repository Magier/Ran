<script lang="ts">
	import Icon from '@iconify/svelte';
	import { iconMap } from '$lib/tactic_icons';
	import type { ScoredCandidate } from '$lib/api';
	import { getCampaignState } from '$lib/components/CampaignState.svelte';

	type RecommendationsProps = {
		class?: string;
		/** Execute a recommendation against its own target. */
		run: (rec: ScoredCandidate) => void;
	};

	let { class: className = '', run }: RecommendationsProps = $props();

	const campaign = getCampaignState();

	let recommendations: ScoredCandidate[] = $state([]);
	let isLoading: boolean = $state(false);
	let collapsed: boolean = $state(false);
	let expandedKey: string | null = $state(null);
	// Current "show count" selection; 0 means "All" (no limit).
	let limitChoice: number = $state(8);

	const keyOf = (rec: ScoredCandidate) => `${rec.ttp_id}::${rec.target_id}`;

	function ttpName(id: string): string {
		return campaign.getTtpById(id)?.name ?? id;
	}
	function ttpTactic(id: string): string | undefined {
		return campaign.getTtpById(id)?.tactic;
	}
	function targetName(id: string): string {
		return campaign.getEntityById(id)?.name ?? id;
	}

	// Refetch on mount and whenever campaign facts change (the graph is
	// reassigned on every `facts-changed` SSE event, so reading it here
	// re-triggers the effect after actions land or new entities are found).
	$effect(() => {
		// Track the signals that should invalidate recommendations.
		void campaign.graph;
		void campaign.entities;
		const max = limitChoice === 0 ? undefined : limitChoice;

		isLoading = true;
		campaign.api
			.GetRecommendations(undefined, max)
			.then((recs: ScoredCandidate[]) => {
				recommendations = recs;
			})
			.catch((err: unknown) => {
				console.error('Failed to fetch recommendations:', err);
				recommendations = [];
			})
			.finally(() => {
				isLoading = false;
			});
	});

	function toggleExpanded(rec: ScoredCandidate) {
		const k = keyOf(rec);
		expandedKey = expandedKey === k ? null : k;
	}

	// Utility bar tint: low → surface, mid → warning, high → success.
	function utilityClass(u: number): string {
		if (u >= 0.66) return 'bg-success-500';
		if (u >= 0.33) return 'bg-warning-500';
		return 'bg-surface-400-600';
	}
</script>

<div class="bg-surface-100-900 border border-surface-200-800 rounded shadow-lg flex flex-col {className}">
	<!-- Header -->
	<div class="flex items-center gap-2 px-3 py-2 border-b border-surface-200-800">
		<Icon icon="mdi:lightbulb-on-outline" width="18" class="text-warning-500 flex-shrink-0" />
		<button
			class="text-sm font-semibold flex-1 text-left hover:text-primary-500"
			onclick={() => (collapsed = !collapsed)}
			title={collapsed ? 'Expand recommendations' : 'Collapse recommendations'}
		>
			Recommended Next Steps
		</button>

		<!-- Show-count selector -->
		<select
			class="text-xs bg-surface-200-800 border border-surface-300-700 rounded px-1 py-0.5"
			bind:value={limitChoice}
			title="Number of recommendations to show"
			aria-label="Number of recommendations to show"
		>
			<option value={8}>8</option>
			<option value={25}>25</option>
			<option value={50}>50</option>
			<option value={0}>All</option>
		</select>

		<span class="text-xs text-surface-500 w-6 text-right">{recommendations.length}</span>
		<button
			onclick={() => (collapsed = !collapsed)}
			aria-label={collapsed ? 'Expand recommendations' : 'Collapse recommendations'}
		>
			<Icon
				icon={collapsed ? 'mdi:chevron-down' : 'mdi:chevron-up'}
				width="16"
				class="text-surface-500"
			/>
		</button>
	</div>

	{#if !collapsed}
		<div class="overflow-y-auto max-h-[60vh] flex flex-col">
			{#if isLoading && recommendations.length === 0}
				<div class="px-3 py-4 text-xs text-surface-500">Scoring actions…</div>
			{:else if recommendations.length === 0}
				<div class="px-3 py-4 text-xs text-surface-500">
					No applicable actions yet. Gain a foothold to get recommendations.
				</div>
			{:else}
				{#each recommendations as rec, i (keyOf(rec))}
					{@const expanded = expandedKey === keyOf(rec)}
					<div class="border-b border-surface-200-800 last:border-b-0">
						<div class="flex items-start gap-2 px-3 py-2">
							<span class="text-xs text-surface-500 w-4 text-right pt-0.5">{i + 1}</span>
							<Icon
								icon={iconMap[ttpTactic(rec.ttp_id) ?? ''] ?? 'mdi:flash'}
								width="18"
								class="flex-shrink-0 mt-0.5"
							/>
							<div class="flex-1 min-w-0">
								<div class="flex items-center gap-2">
									<span class="text-sm truncate" title={ttpName(rec.ttp_id)}>{ttpName(rec.ttp_id)}</span>
									<span class="text-xs text-surface-500 ml-auto flex-shrink-0">{Math.round(rec.utility * 100)}</span>
								</div>
								<!-- Utility bar -->
								<div class="h-1.5 mt-1 rounded bg-surface-300-700 overflow-hidden">
									<div class="h-full {utilityClass(rec.utility)}" style="width: {Math.round(rec.utility * 100)}%"></div>
								</div>
								<div class="flex items-center gap-2 mt-1">
									<button
										class="text-xs text-surface-500 hover:text-primary-500 truncate"
										title="Focus {targetName(rec.target_id)}"
										onclick={() => toggleExpanded(rec)}
									>
										→ {targetName(rec.target_id)}
									</button>
									<button
										class="text-xs text-surface-500 hover:text-primary-500 ml-auto flex items-center gap-0.5"
										onclick={() => toggleExpanded(rec)}
									>
										why
										<Icon icon={expanded ? 'mdi:chevron-up' : 'mdi:chevron-down'} width="12" />
									</button>
								</div>
							</div>
							<button
								class="btn btn-sm preset-filled-primary-500 text-xs px-2 py-1 flex-shrink-0"
								title="Execute against {targetName(rec.target_id)}"
								onclick={() => run(rec)}
							>
								Run
							</button>
						</div>

						{#if expanded}
							<!-- Per-consideration breakdown -->
							<div class="px-3 pb-2 pl-9 space-y-1">
								{#each rec.breakdown as c (c.name)}
									<div class="flex items-center gap-2">
										<span class="text-xs text-surface-500 w-28 truncate" title={c.name}>{c.name}</span>
										<div class="h-1 flex-1 rounded bg-surface-300-700 overflow-hidden">
											<div
												class="h-full {c.veto ? 'bg-error-500' : 'bg-primary-500'}"
												style="width: {Math.round(c.curved * 100)}%"
											></div>
										</div>
										<span class="text-xs text-surface-500 w-7 text-right">{c.curved.toFixed(2)}</span>
										{#if c.veto}
											<span class="text-[10px] text-error-500" title="Veto consideration">veto</span>
										{:else if c.weight !== 1}
											<span class="text-[10px] text-surface-500" title="Weight">×{c.weight}</span>
										{/if}
									</div>
								{/each}
							</div>
						{/if}
					</div>
				{/each}
			{/if}
		</div>
	{/if}
</div>
