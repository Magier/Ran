<script lang="ts">
	import Icon from '@iconify/svelte';
	import { DEFAULT_LAYOUT_PARAMS } from './elk_layout';
	import type { LayoutParams, LayeringStrategy, NodePlacementStrategy } from './elk_layout';

	type Props = {
		params: LayoutParams;
		onRelayout: () => void;
	};

	let { params = $bindable(), onRelayout }: Props = $props();

	let panelOpen = $state(false);
	let debounceTimer: ReturnType<typeof setTimeout> | null = null;

	type Slider = { key: keyof LayoutParams; label: string; min: number; max: number; step: number; unit: string };
	type Dropdown<T extends string> = { key: keyof LayoutParams; label: string; options: { value: T; label: string }[] };

	const spacingSliders: Slider[] = [
		{ key: 'layerSpacing',   label: 'Layer spacing',      min: 0, max: 400, step: 5,   unit: 'px' },
		{ key: 'nodeSpacing',    label: 'Node spacing',        min: 0, max: 150, step: 5,   unit: 'px' },
		{ key: 'edgeNodeSpacing',label: 'Edge-node spacing',   min: 0, max: 150, step: 5,   unit: 'px' },
		{ key: 'aspectRatio',    label: 'Aspect ratio',        min: 0, max: 5,   step: 0.1, unit: '' },
	];

	const compoundSliders: Slider[] = [
		{ key: 'compoundEdgeLength', label: 'Edge length',   min: 0, max: 200, step: 5,  unit: 'px' },
		{ key: 'compoundPadding',    label: 'Padding',       min: 0, max: 60,  step: 5,  unit: 'px' },
		{ key: 'stressIterations',   label: 'Iterations',    min: 0, max: 1000, step: 10, unit: '' },
	];

	const animSliders: Slider[] = [
		{ key: 'animationDuration', label: 'Duration', min: 0, max: 2000, step: 50, unit: 'ms' },
	];

	const layeringDropdown: Dropdown<LayeringStrategy> = {
		key: 'layeringStrategy',
		label: 'Layering',
		options: [
			{ value: 'NETWORK_SIMPLEX', label: 'Network simplex' },
			{ value: 'LONGEST_PATH',    label: 'Longest path' },
			{ value: 'INTERACTIVE',     label: 'Interactive' },
			{ value: 'MIN_WIDTH',       label: 'Min width' },
		],
	};

	const placementDropdown: Dropdown<NodePlacementStrategy> = {
		key: 'nodePlacementStrategy',
		label: 'Node placement',
		options: [
			{ value: 'BRANDES_KOEPF',    label: 'Brandes-Koepf' },
			{ value: 'NETWORK_SIMPLEX',  label: 'Network simplex' },
			{ value: 'LINEAR_SEGMENTS',  label: 'Linear segments' },
			{ value: 'SIMPLE',           label: 'Simple' },
		],
	};

	function onChange() {
		if (debounceTimer) clearTimeout(debounceTimer);
		debounceTimer = setTimeout(() => onRelayout(), 400);
	}

	function reset() {
		params = { ...DEFAULT_LAYOUT_PARAMS };
		onChange();
	}
</script>

{#if panelOpen}
	<div class="fixed inset-0 z-40" role="presentation" onclick={() => (panelOpen = false)}></div>
{/if}

<div class="absolute bottom-1 right-12 z-50 text-surface-700-300">
	<button
		class="chip preset-outlined-surface-100-900 border-surface-400-600"
		onclick={() => (panelOpen = !panelOpen)}
		title="Layout playground"
		aria-label="Toggle layout playground"
	>
		<Icon icon="mdi:tune-variant" class="inline-block text-surface-400-600" />
	</button>

	{#if panelOpen}
		<div
			class="absolute bottom-full right-0 mb-1 w-72 bg-surface-50-950 border border-surface-300-700 rounded-lg shadow-xl z-50 font-mono overflow-y-auto max-h-[80vh]"
			role="dialog"
			aria-label="Layout playground"
			tabindex="-1"
			onclick={(e) => e.stopPropagation()}
			onkeydown={(e) => e.stopPropagation()}
		>
			<!-- Header -->
			<div class="flex items-center justify-between px-4 pt-4 pb-2 sticky top-0 bg-surface-50-950 border-b border-surface-200-800">
				<h3 class="text-xs font-semibold text-primary-500 tracking-widest uppercase">Layout playground</h3>
				<button
					class="text-xs text-surface-400-600 hover:text-surface-700-300 transition-colors cursor-pointer"
					onclick={reset}
				>reset</button>
			</div>

			<div class="p-4 space-y-5">

				<!-- Strategies -->
				<section>
					<p class="text-[10px] text-surface-400-600 uppercase tracking-widest mb-2">Strategies</p>
					{#each [layeringDropdown, placementDropdown] as d}
						<div class="mb-2">
							<label for="layout-{String(d.key)}" class="text-xs text-surface-500-400 block mb-1">{d.label}</label>
							<select
								id="layout-{String(d.key)}"
								bind:value={params[d.key]}
								onchange={onChange}
								class="w-full text-xs bg-surface-100-900 border border-surface-300-700 rounded px-2 py-1 text-surface-700-300 cursor-pointer"
							>
								{#each d.options as opt}
									<option value={opt.value}>{opt.label}</option>
								{/each}
							</select>
						</div>
					{/each}
				</section>

				<!-- Global spacing -->
				<section>
					<p class="text-[10px] text-surface-400-600 uppercase tracking-widest mb-2">Global spacing</p>
					{#each spacingSliders as s}
						<div class="mb-3">
							<div class="flex justify-between items-baseline mb-1">
								<label for="layout-{s.key}" class="text-xs text-surface-500-400">{s.label}</label>
								<span class="text-xs text-primary-400 tabular-nums">{params[s.key]}{s.unit}</span>
							</div>
							<input
								id="layout-{s.key}"
								type="range"
								min={s.min}
								max={s.max}
								step={s.step}
								bind:value={params[s.key]}
								oninput={onChange}
								class="w-full h-1 appearance-none bg-surface-300-700 rounded-full cursor-pointer accent-primary-500"
							/>
							<div class="flex justify-between text-[10px] text-surface-400-600 mt-0.5">
								<span>{s.min}</span><span>{s.max}</span>
							</div>
						</div>
					{/each}
				</section>

				<!-- Compound / cluster -->
				<section>
					<p class="text-[10px] text-surface-400-600 uppercase tracking-widest mb-2">Cluster (stress)</p>
					{#each compoundSliders as s}
						<div class="mb-3">
							<div class="flex justify-between items-baseline mb-1">
								<label for="layout-{s.key}" class="text-xs text-surface-500-400">{s.label}</label>
								<span class="text-xs text-primary-400 tabular-nums">{params[s.key]}{s.unit}</span>
							</div>
							<input
								id="layout-{s.key}"
								type="range"
								min={s.min}
								max={s.max}
								step={s.step}
								bind:value={params[s.key]}
								oninput={onChange}
								class="w-full h-1 appearance-none bg-surface-300-700 rounded-full cursor-pointer accent-primary-500"
							/>
							<div class="flex justify-between text-[10px] text-surface-400-600 mt-0.5">
								<span>{s.min}</span><span>{s.max}</span>
							</div>
						</div>
					{/each}
				</section>

				<!-- Animation -->
				<section>
					<p class="text-[10px] text-surface-400-600 uppercase tracking-widest mb-2">Animation</p>
					{#each animSliders as s}
						<div class="mb-3">
							<div class="flex justify-between items-baseline mb-1">
								<label for="layout-{s.key}" class="text-xs text-surface-500-400">{s.label}</label>
								<span class="text-xs text-primary-400 tabular-nums">{params[s.key]}{s.unit}</span>
							</div>
							<input
								id="layout-{s.key}"
								type="range"
								min={s.min}
								max={s.max}
								step={s.step}
								bind:value={params[s.key]}
								oninput={onChange}
								class="w-full h-1 appearance-none bg-surface-300-700 rounded-full cursor-pointer accent-primary-500"
							/>
							<div class="flex justify-between text-[10px] text-surface-400-600 mt-0.5">
								<span>{s.min}</span><span>{s.max}</span>
							</div>
						</div>
					{/each}
				</section>

			</div>
		</div>
	{/if}
</div>
