<script lang="ts">
	import type { PlanSummary } from '$lib/api';

	interface PlanPickerProps {
		plans: PlanSummary[];
		loading?: boolean;
		onLoad: (plan: PlanSummary) => void;
		onClose: () => void;
	}

	let { plans, loading = false, onLoad, onClose }: PlanPickerProps = $props();
</script>

<div class="space-y-4">
	<header class="flex items-center justify-between">
		<h3 class="text-lg font-bold">Load Plan</h3>
		<button class="btn preset-outlined-surface-500 btn-sm" onclick={onClose}>✕</button>
	</header>

	<div class="max-h-[60vh] space-y-2 overflow-auto">
		{#if loading}
			<p class="p-4 text-center opacity-70">Loading plans…</p>
		{:else if plans.length === 0}
			<p class="p-4 text-center opacity-70">
				No plans found. Add <code>*.plan.yaml</code> files to the configured plans directory.
			</p>
		{:else}
			{#each plans as plan (plan.filename)}
				<div class="bg-surface-200-800 flex items-center justify-between gap-4 rounded-md p-3">
					<div class="min-w-0">
						<p class="truncate font-semibold" title={plan.name}>{plan.name}</p>
						{#if plan.description}
							<p class="truncate text-sm opacity-70" title={plan.description}>
								{plan.description}
							</p>
						{/if}
						<p class="truncate text-xs opacity-50" title={plan.filename}>
							{plan.filename} · {plan.steps} step{plan.steps === 1 ? '' : 's'}
						</p>
					</div>
					<button
						class="btn preset-filled-primary-500 btn-sm shrink-0"
						onclick={() => onLoad(plan)}
					>
						Load
					</button>
				</div>
			{/each}
		{/if}
	</div>

	<footer class="flex justify-end">
		<button class="btn preset-filled-surface-500" onclick={onClose}>Close</button>
	</footer>
</div>
