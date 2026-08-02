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
		<h3 class="font-bold text-lg">Load Plan</h3>
		<button class="btn preset-outlined-surface-500 btn-sm" onclick={onClose}>✕</button>
	</header>

	<div class="max-h-[60vh] overflow-auto space-y-2">
		{#if loading}
			<p class="opacity-70 p-4 text-center">Loading plans…</p>
		{:else if plans.length === 0}
			<p class="opacity-70 p-4 text-center">
				No plans found. Add <code>*.plan.yaml</code> files to the configured plans directory.
			</p>
		{:else}
			{#each plans as plan (plan.filename)}
				<div
					class="flex items-center justify-between gap-4 rounded-md bg-surface-200-800 p-3"
				>
					<div class="min-w-0">
						<p class="font-semibold truncate" title={plan.name}>{plan.name}</p>
						{#if plan.description}
							<p class="text-sm opacity-70 truncate" title={plan.description}>
								{plan.description}
							</p>
						{/if}
						<p class="text-xs opacity-50 truncate" title={plan.filename}>
							{plan.filename} · {plan.steps} step{plan.steps === 1 ? '' : 's'}
						</p>
					</div>
					<button class="btn preset-filled-primary-500 btn-sm shrink-0" onclick={() => onLoad(plan)}>
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
