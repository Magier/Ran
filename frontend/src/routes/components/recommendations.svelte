<script lang="ts">
	import type { Snippet } from 'svelte';
	import Icon from '@iconify/svelte';
	import { iconMap } from '$lib/tactic_icons';
	import type { ScoredCandidate, ConsiderationScore } from '$lib/api';
	import { getCampaignState } from '$lib/components/CampaignState.svelte';
	import ConsiderationBreakdown from './consideration_breakdown.svelte';

	type RecommendationsProps = {
		class?: string;
		/** Execute a recommendation against its own target. */
		run: (rec: ScoredCandidate) => void;
		/** Optional controls rendered at the right of the toolbar (e.g. the scoring tuner). */
		actions?: Snippet;
	};

	let { class: className = '', run, actions }: RecommendationsProps = $props();

	const campaign = getCampaignState();

	// One action scored identically across one or more targets. Targets that
	// yield the same utility for the same TTP are folded into a single row so the
	// list isn't flooded with near-duplicates.
	type GroupedRec = {
		ttp_id: string;
		utility: number;
		utility_score: number;
		success_probability: number;
		breakdown: ConsiderationScore[];
		targets: string[];
	};

	let recommendations: ScoredCandidate[] = $state([]);
	let isLoading: boolean = $state(false);
	let expandedKey: string | null = $state(null);
	// Current "show count" selection; 0 means "All" (no limit). Counts grouped rows.
	let limitChoice: number = $state(20);

	const keyOf = (g: GroupedRec) => `${g.ttp_id}::${g.utility}`;

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
	// We fetch the full set and group/limit client-side, so the limit selector
	// doesn't trigger a refetch.
	$effect(() => {
		// Track the signals that should invalidate recommendations.
		void campaign.graph;
		void campaign.entities;
		void campaign.scoringVersion;

		isLoading = true;
		campaign.api
			.GetRecommendations(undefined, undefined)
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

	// Fold same-TTP/same-utility candidates across targets into one group.
	let groups: GroupedRec[] = $derived.by(() => {
		const byKey = new Map<string, GroupedRec>();
		for (const r of recommendations) {
			const key = `${r.ttp_id}::${r.utility.toFixed(4)}`;
			const existing = byKey.get(key);
			if (existing) {
				existing.targets.push(r.target_id);
			} else {
				byKey.set(key, {
					ttp_id: r.ttp_id,
					utility: r.utility,
					utility_score: r.utility_score,
					success_probability: r.success_probability,
					breakdown: r.breakdown,
					targets: [r.target_id]
				});
			}
		}
		return Array.from(byKey.values()).sort((a, b) => b.utility - a.utility);
	});

	let shownGroups: GroupedRec[] = $derived(
		limitChoice === 0 ? groups : groups.slice(0, limitChoice)
	);

	function toggleExpanded(g: GroupedRec) {
		const k = keyOf(g);
		expandedKey = expandedKey === k ? null : k;
	}

	// Run this action against one specific target from the group.
	function runTarget(g: GroupedRec, targetId: string) {
		run({
			ttp_id: g.ttp_id,
			target_id: targetId,
			utility: g.utility,
			utility_score: g.utility_score,
			success_probability: g.success_probability,
			breakdown: g.breakdown
		});
	}

	// Utility bar tint: low → surface, mid → warning, high → success.
	function utilityClass(u: number): string {
		if (u >= 0.66) return 'bg-success-500';
		if (u >= 0.33) return 'bg-warning-500';
		return 'bg-surface-400-600';
	}
</script>

<div class="bg-surface-100-900 flex h-full min-h-0 flex-col {className}">
	<!-- Toolbar: title + show-count, mirroring the Actions tab's toolbar row -->
	<div class="border-surface-200-800 flex flex-shrink-0 items-center gap-2 border-b px-2 py-2">
		<Icon icon="mdi:lightbulb-on-outline" width="16" class="text-warning-500 flex-shrink-0" />
		<span class="flex-1 truncate text-sm font-semibold">Recommended Next Steps</span>
		<select
			class="bg-surface-200-800 border-surface-300-700 rounded border px-1 py-0.5 text-xs"
			bind:value={limitChoice}
			title="Number of recommendations to show"
			aria-label="Number of recommendations to show"
		>
			<option value={20}>20</option>
			<option value={50}>50</option>
			<option value={100}>100</option>
			<option value={0}>All</option>
		</select>
		<span class="text-surface-500 w-6 text-right text-xs" title="{shownGroups.length} shown">
			{shownGroups.length}
		</span>
		{#if actions}
			<div class="flex items-center">{@render actions()}</div>
		{/if}
	</div>

	<div class="flex min-h-0 flex-1 flex-col overflow-y-auto">
		{#if isLoading && groups.length === 0}
			<div class="text-surface-500 px-3 py-4 text-xs">Scoring actions…</div>
		{:else if groups.length === 0}
			<div class="text-surface-500 px-3 py-4 text-xs">
				No applicable actions yet. Gain a foothold to get recommendations.
			</div>
		{:else}
			{#each shownGroups as g, i (keyOf(g))}
				{@const expanded = expandedKey === keyOf(g)}
				{@const multi = g.targets.length > 1}
				<div class="border-surface-200-800 border-b last:border-b-0">
					<div class="flex items-start gap-2 px-3 py-2">
						<span class="text-surface-500 w-4 pt-0.5 text-right text-xs">{i + 1}</span>
						<Icon
							icon={iconMap[ttpTactic(g.ttp_id) ?? ''] ?? 'mdi:flash'}
							width="18"
							class="mt-0.5 flex-shrink-0"
						/>
						<div class="min-w-0 flex-1">
							<div class="flex items-center gap-2">
								<span class="truncate text-sm" title={ttpName(g.ttp_id)}>{ttpName(g.ttp_id)}</span>
								<span class="text-surface-500 ml-auto flex-shrink-0 text-xs"
									>{Math.round(g.utility * 100)}</span
								>
							</div>
							<!-- Utility bar -->
							<div class="bg-surface-300-700 mt-1 h-1.5 overflow-hidden rounded">
								<div
									class="h-full {utilityClass(g.utility)}"
									style="width: {Math.round(g.utility * 100)}%"
								></div>
							</div>
							<div class="mt-1 flex items-center gap-2">
								<button
									class="text-surface-500 hover:text-primary-500 truncate text-xs"
									title={multi
										? `${g.targets.length} targets, same utility`
										: `Focus ${targetName(g.targets[0])}`}
									onclick={() => toggleExpanded(g)}
								>
									{#if multi}
										→ {g.targets.length} targets
									{:else}
										→ {targetName(g.targets[0])}
									{/if}
								</button>
								<button
									class="text-surface-500 hover:text-primary-500 ml-auto flex items-center gap-0.5 text-xs"
									onclick={() => toggleExpanded(g)}
								>
									why
									<Icon icon={expanded ? 'mdi:chevron-up' : 'mdi:chevron-down'} width="12" />
								</button>
							</div>
						</div>
						{#if !multi}
							<button
								class="btn btn-sm preset-filled-primary-500 flex-shrink-0 px-2 py-1 text-xs"
								title="Execute against {targetName(g.targets[0])}"
								onclick={() => runTarget(g, g.targets[0])}
							>
								Run
							</button>
						{/if}
					</div>

					{#if expanded}
						{#if multi}
							<!-- Per-target run list (all share the same utility) -->
							<div class="space-y-1 px-3 pb-2 pl-9">
								{#each g.targets as t (t)}
									<div class="flex items-center gap-2">
										<span class="text-surface-500 flex-1 truncate text-xs" title={targetName(t)}>
											→ {targetName(t)}
										</span>
										<button
											class="btn btn-sm preset-filled-primary-500 flex-shrink-0 px-2 py-0.5 text-xs"
											title="Execute against {targetName(t)}"
											onclick={() => runTarget(g, t)}
										>
											Run
										</button>
									</div>
								{/each}
							</div>
						{/if}
						<!-- Per-consideration breakdown -->
						<ConsiderationBreakdown breakdown={g.breakdown} class="px-3 pb-2 pl-9" />
					{/if}
				</div>
			{/each}
		{/if}
	</div>
</div>
