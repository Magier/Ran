<script lang="ts">
	import type { ConsiderationScore } from '$lib/api';

	type Props = {
		/** Per-consideration scoring that produced a candidate's utility. */
		breakdown: ConsiderationScore[];
		class?: string;
	};

	let { breakdown, class: className = '' }: Props = $props();

	function isActiveVeto(c: ConsiderationScore): boolean {
		return c.kind === 'utility' && c.veto && c.curved <= 0;
	}

	function isBeliefBlocker(c: ConsiderationScore): boolean {
		return c.kind === 'belief' && c.curved <= 0;
	}

	function statusTitle(c: ConsiderationScore): string {
		if (isActiveVeto(c)) return 'Vetoed: this configured veto zeroed the utility score';
		if (c.veto) return 'Veto: configured as a multiplicative gate and currently passing';
		if (isBeliefBlocker(c)) {
			return 'Blocked: this belief factor made success probability zero; it is not a configured veto';
		}
		if (c.kind === 'belief') return 'Belief factor: multiplies into success probability';
		if (c.weight !== 1) return `Utility consideration with weight ${c.weight}`;
		return 'Utility consideration';
	}

</script>

<div class="space-y-1 {className}">
	{#each breakdown as c (c.name)}
		<div
			class="flex items-center gap-2 rounded px-1 -mx-1 {isActiveVeto(c) || isBeliefBlocker(c) ? 'bg-error-500/10' : ''}"
		>
			<span class="text-xs text-surface-500 w-28 truncate" title={`${c.name} - ${statusTitle(c)}`}>{c.name}</span>
			<div class="h-1 flex-1 rounded bg-surface-300-700 overflow-hidden">
				<div
					class="h-full {c.veto || isBeliefBlocker(c) ? 'bg-error-500' : c.kind === 'belief' ? 'bg-warning-500' : 'bg-primary-500'}"
					style="width: {Math.round(c.curved * 100)}%"
				></div>
			</div>
			<span class="text-xs text-surface-500 w-7 text-right">{c.curved.toFixed(2)}</span>
		</div>
	{/each}
</div>
