<script lang="ts">
	import Icon from '@iconify/svelte';
	import { getCampaignState } from '$lib/components/CampaignState.svelte';
	import type {
		ScoringProfile,
		NamedConsideration,
		ResponseCurve,
		CombinationMode,
		CalibrationResult
	} from '$lib/api';

	const campaign = getCampaignState();

	let profile: ScoringProfile | null = $state(null);
	let open: boolean = $state(false);
	let saving: boolean = $state(false);
	// Pending calibration preview (fitted from captured operator decisions),
	// shown for review before the operator applies it.
	let calibration: CalibrationResult | null = $state(null);
	let calibrating: boolean = $state(false);
	let calibrateError: string | null = $state(null);

	// Load the live profile on mount; the component renders nothing unless the
	// tuning feature flag is on.
	$effect(() => {
		campaign.api.GetScoringProfile().then((p) => {
			profile = p;
		});
	});

	const curveTypes: ResponseCurve['type'][] = ['linear', 'polynomial', 'logistic', 'step'];
	const combinationModes: CombinationMode[] = [
		'weighted_arithmetic',
		'weighted_geometric',
		'iaus_multiplicative'
	];

	// Sensible defaults when switching a consideration to a new curve type.
	function defaultCurve(type: ResponseCurve['type']): ResponseCurve {
		switch (type) {
			case 'linear':
				return { type, slope: 1, intercept: 0 };
			case 'polynomial':
				return { type, exponent: 2, slope: 1, intercept: 0 };
			case 'logistic':
				return { type, steepness: 10, midpoint: 0.5 };
			case 'step':
				return { type, threshold: 0.5 };
		}
	}

	// Evaluate a curve at x∈[0,1], mirroring Rust `ResponseCurve::apply` (clamped).
	function applyCurve(c: ResponseCurve, x: number): number {
		const xc = Math.min(1, Math.max(0, x));
		let y: number;
		switch (c.type) {
			case 'linear':
				y = (c.slope ?? 1) * xc + (c.intercept ?? 0);
				break;
			case 'polynomial':
				y = (c.slope ?? 1) * Math.pow(xc, c.exponent ?? 1) + (c.intercept ?? 0);
				break;
			case 'logistic':
				y = 1 / (1 + Math.exp(-(c.steepness ?? 1) * (xc - (c.midpoint ?? 0.5))));
				break;
			case 'step':
				y = xc >= (c.threshold ?? 0.5) ? 1 : 0;
				break;
			default:
				y = xc;
		}
		return Math.min(1, Math.max(0, y));
	}

	const PLOT_W = 130;
	const PLOT_H = 56;

	// Polyline points for a curve plot (y inverted for SVG coords).
	function plotPoints(c: ResponseCurve): string {
		const n = 48;
		const pts: string[] = [];
		for (let i = 0; i <= n; i++) {
			const x = i / n;
			const y = applyCurve(c, x);
			pts.push(`${(x * PLOT_W).toFixed(1)},${((1 - y) * PLOT_H).toFixed(1)}`);
		}
		return pts.join(' ');
	}

	// Numeric param fields shown for each curve type.
	function paramKeys(type: ResponseCurve['type']): string[] {
		switch (type) {
			case 'linear':
				return ['slope', 'intercept'];
			case 'polynomial':
				return ['exponent', 'slope', 'intercept'];
			case 'logistic':
				return ['steepness', 'midpoint'];
			case 'step':
				return ['threshold'];
		}
	}

	// Debounced PUT so dragging/typing doesn't hammer the endpoint.
	let saveTimer: ReturnType<typeof setTimeout> | undefined;
	function scheduleSave() {
		if (saveTimer) clearTimeout(saveTimer);
		saveTimer = setTimeout(save, 200);
	}

	async function save() {
		if (!profile) return;
		saving = true;
		const updated = await campaign.api.UpdateScoringProfile({
			combination: profile.combination,
			considerations: profile.considerations
		});
		saving = false;
		if (updated) {
			profile = updated;
			// Signal recommendation views to refetch with the new profile.
			campaign.scoringVersion += 1;
		}
	}

	function setCurveType(c: NamedConsideration, type: ResponseCurve['type']) {
		c.curve = defaultCurve(type);
		scheduleSave();
	}

	// Persist the current live profile to the sidecar (survives restart).
	async function persist() {
		const saved = await campaign.api.SaveScoringProfile();
		if (saved) profile = saved;
	}

	// Revert to the configured base profile and drop persisted overrides.
	async function reset() {
		const base = await campaign.api.ResetScoringProfile();
		if (base) {
			profile = base;
			campaign.scoringVersion += 1;
		}
	}

	// Fit weights from the operator decisions captured so far. Shows a preview;
	// the operator reviews the fit quality before applying.
	async function calibrate() {
		calibrating = true;
		calibrateError = null;
		const result = await campaign.api.CalibrateScoring();
		calibrating = false;
		if (result) {
			calibration = result;
		} else {
			calibrateError = 'No captured decisions yet — execute some actions first.';
		}
	}

	// Apply the previewed calibration to the live profile (via the normal update
	// path, so it flows through the same save/version bump as manual edits).
	async function applyCalibration() {
		if (!calibration) return;
		profile = { ...calibration.profile };
		calibration = null;
		await save();
	}

	function pct(x: number): string {
		return `${Math.round(x * 100)}%`;
	}
</script>

{#if profile?.tuningEnabled}
	<!-- Trigger -->
	<button
		class="bg-surface-200-800 hover:bg-surface-300-700 border border-surface-400-600 rounded-full p-2 shadow-lg"
		title="Tune scoring"
		aria-label="Tune scoring"
		onclick={() => (open = !open)}
	>
		<Icon icon="mdi:tune-variant" width="18" />
	</button>

	<!-- Flyout -->
	{#if open}
		<div
			class="fixed inset-y-0 right-0 z-[80] w-[380px] max-w-[90vw] bg-surface-100-900 border-l border-surface-300-700 shadow-2xl flex flex-col"
		>
			<div class="flex items-center gap-2 px-3 py-2 border-b border-surface-200-800 shrink-0">
				<Icon icon="mdi:tune-variant" width="18" class="text-primary-500" />
				<span class="text-sm font-semibold flex-1">Scoring Tuner</span>
				{#if saving}<span class="text-xs text-surface-500">saving…</span>{/if}
				<button
					class="text-xs px-2 py-0.5 rounded bg-surface-200-800 hover:bg-surface-300-700 border border-surface-400-600"
					title="Persist to ran.scoring.yaml (survives restart)"
					onclick={persist}
				>
					Save
				</button>
				<button
					class="text-xs px-2 py-0.5 rounded bg-surface-200-800 hover:bg-surface-300-700 border border-surface-400-600"
					title="Revert to configured defaults and drop saved overrides"
					onclick={reset}
				>
					Reset
				</button>
				<button
					class="text-xs px-2 py-0.5 rounded bg-primary-500/20 hover:bg-primary-500/30 border border-primary-500/50 disabled:opacity-50"
					title="Fit weights from the operator decisions captured this and prior sessions"
					disabled={calibrating}
					onclick={calibrate}
				>
					{calibrating ? 'Calibrating…' : 'Calibrate'}
				</button>
				<button aria-label="Close" onclick={() => (open = false)}>
					<Icon icon="mdi:close" width="18" class="text-surface-500 hover:text-error-500" />
				</button>
			</div>

			{#if calibrateError}
				<div class="px-3 py-2 text-xs text-error-400 border-b border-surface-200-800 shrink-0">
					{calibrateError}
				</div>
			{/if}

			{#if calibration}
				{@const m = calibration.metrics}
				<div class="px-3 py-2 border-b border-surface-200-800 shrink-0 bg-primary-500/5 space-y-1.5">
					<div class="flex items-center gap-2">
						<Icon icon="mdi:auto-fix" width="15" class="text-primary-500" />
						<span class="text-xs font-semibold flex-1">Calibration preview</span>
						<button
							class="text-xs px-2 py-0.5 rounded bg-primary-500 text-white hover:bg-primary-600"
							title="Apply the fitted weights to the live profile"
							onclick={applyCalibration}
						>
							Apply
						</button>
						<button
							class="text-xs px-2 py-0.5 rounded bg-surface-200-800 hover:bg-surface-300-700 border border-surface-400-600"
							onclick={() => (calibration = null)}
						>
							Dismiss
						</button>
					</div>
					<div class="grid grid-cols-2 gap-x-3 gap-y-0.5 text-xs text-surface-600-400">
						<span title="Fraction of decisions where the operator's choice ranks first"
							>Match (top-1): <span class="font-mono text-surface-900-100">{pct(m.top1Accuracy)}</span></span
						>
						<span title="Mean probability the fitted model assigns the operator's choices"
							>Confidence: <span class="font-mono text-surface-900-100">{pct(m.meanChosenProb)}</span></span
						>
						<span>Decisions: <span class="font-mono text-surface-900-100">{m.decisions}</span></span>
						<span
							title="Choices no non-negative weighting can reproduce — a missing consideration"
							>Unreproducible:
							<span
								class="font-mono {m.infeasible > 0 ? 'text-warning-500' : 'text-surface-900-100'}"
								>{m.infeasible}</span
							></span
						>
					</div>
					{#if m.infeasible > 0}
						<p class="text-[11px] text-warning-500/90 leading-snug">
							{m.infeasible} decision{m.infeasible === 1 ? '' : 's'} can't be reproduced by any weighting
							— the operator valued something the current considerations don't measure.
						</p>
					{/if}
				</div>
			{/if}

			<div class="overflow-y-auto flex-1 p-3 space-y-3">
				<!-- Combination mode -->
				<label class="flex items-center gap-2 text-xs">
					<span class="text-surface-500 w-24">combination</span>
					<select
						class="flex-1 bg-surface-200-800 border border-surface-300-700 rounded px-1 py-0.5"
						bind:value={profile.combination}
						onchange={scheduleSave}
					>
						{#each combinationModes as m}
							<option value={m}>{m}</option>
						{/each}
					</select>
				</label>

				{#each profile.considerations as c (c.name)}
					<div
						class="border border-surface-200-800 rounded p-2 space-y-2"
						class:opacity-50={!c.enabled}
					>
						<div class="flex items-center gap-2">
							<span class="text-sm font-medium flex-1 truncate">{c.name}</span>
							<label class="flex items-center gap-1 text-[10px] text-surface-500" title="Veto gate">
								<input type="checkbox" bind:checked={c.veto} onchange={scheduleSave} />
								veto
							</label>
							<label class="flex items-center gap-1 text-[10px] text-surface-500" title="Enabled">
								<input type="checkbox" bind:checked={c.enabled} onchange={scheduleSave} />
								on
							</label>
						</div>

						<div class="flex gap-2">
							<!-- Curve plot -->
							<svg
								width={PLOT_W}
								height={PLOT_H}
								class="shrink-0 bg-surface-200-800 rounded border border-surface-300-700"
							>
								<!-- identity reference -->
								<line
									x1="0"
									y1={PLOT_H}
									x2={PLOT_W}
									y2="0"
									stroke="currentColor"
									stroke-width="0.5"
									class="text-surface-500"
									stroke-dasharray="2 2"
								/>
								<polyline
									points={plotPoints(c.curve)}
									fill="none"
									stroke="currentColor"
									stroke-width="1.5"
									class={c.veto ? 'text-error-500' : 'text-primary-500'}
								/>
							</svg>

							<!-- Controls -->
							<div class="flex-1 space-y-1">
								<select
									class="w-full text-xs bg-surface-200-800 border border-surface-300-700 rounded px-1 py-0.5"
									value={c.curve.type}
									onchange={(e) =>
										setCurveType(c, e.currentTarget.value as ResponseCurve['type'])}
								>
									{#each curveTypes as t}
										<option value={t}>{t}</option>
									{/each}
								</select>

								{#each paramKeys(c.curve.type) as key}
									<label class="flex items-center gap-1 text-[10px]">
										<span class="text-surface-500 w-16">{key}</span>
										<input
											type="number"
											step="0.1"
											class="num-input flex-1 w-0 h-5 text-[10px] leading-none bg-surface-200-800 border border-surface-300-700 rounded px-1"
											value={(c.curve as unknown as Record<string, number>)[key] ?? 0}
											oninput={(e) => {
												(c.curve as unknown as Record<string, number>)[key] =
													parseFloat(e.currentTarget.value) || 0;
												scheduleSave();
											}}
										/>
									</label>
								{/each}

								<label class="flex items-center gap-1 text-[10px]">
									<span class="text-surface-500 w-16">weight</span>
									<input
										type="range"
										min="0"
										max="3"
										step="0.1"
										class="flex-1"
										bind:value={c.weight}
										onchange={scheduleSave}
									/>
									<span class="text-surface-500 w-6 text-right">{c.weight.toFixed(1)}</span>
								</label>
							</div>
						</div>
					</div>
				{/each}
			</div>
		</div>
	{/if}
{/if}

<style>
	/* Drop native number spinners so the param fields stay compact and in line
	   with the other controls. */
	.num-input::-webkit-outer-spin-button,
	.num-input::-webkit-inner-spin-button {
		-webkit-appearance: none;
		margin: 0;
	}
	.num-input {
		-moz-appearance: textfield;
		appearance: textfield;
	}
</style>
