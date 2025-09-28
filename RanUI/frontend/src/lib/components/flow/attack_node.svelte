<script lang="ts">
	import { Handle, Position, type NodeProps } from '@xyflow/svelte';
	import { campaign } from '$lib/wailsjs/go/models';
	import { iconMap } from '$lib/tactic_icons';
	import Icon from '@iconify/svelte';

	interface ActionNodeData extends Record<string, unknown> {
		step: campaign.AttackStep;
		color: string;
	}

	interface ActionNodeProps extends NodeProps {
		data: ActionNodeData;
	}
	let {
		targetPosition = Position.Left,
		sourcePosition = Position.Right,
		data,
		isConnectable = false
	}: ActionNodeProps = $props();

	const { label, step } = data;

	const { statusBorder, icon: stateIcon, color } = data.step?.Success
		? { statusBorder: 'border-green-500', icon: 'lucide:check', color: 'text-success-500' }
		: { statusBorder: 'border-red-500', icon: 'lucide:x', color: 'text-error-500' };
</script>

<div class={['bg-surface-50-950 border-1 rounded-md border-solid px-2 py-2', statusBorder]}>
		<span class="text-base">{label}</span>
		<pre class="text-xs">{step.Target.name}</pre>

	<div class="w-full flex items-left mt-2">
		<div class="flex">
			<span class="badge bg-surface-100-900 text-tertiary-contrast-200-800">
				<Icon icon={iconMap[step.TTP.tactic]} width="16"/>
				{step.TTP.tactic}
			</span>
		</div>
		<div class="flex-1"></div>
		<div class="flex items-center space-x-1">
			{#if step.Observables?.length > 0}
				<Icon icon={"humbleicons:eye"} width="16" />
			{/if}
			{#if step?.Success}
				<Icon class={color} icon={stateIcon} width="16" />
			{/if}
		</div>
	</div>

	<Handle type="target" position={targetPosition} {isConnectable} />
	<Handle type="source" position={sourcePosition} {isConnectable} />
</div>

<style>
	:global(.svelte-flow__node-actionNode) {
		font-size: 12px;
		border-radius: 4px;
	}
</style>
