<script lang="ts">
	import { Handle, Position, type NodeProps } from '@xyflow/svelte';
	import { campaign } from '$lib/wailsjs/go/models';
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

	const { statusBorder, icon, color } = data.step?.Success
		? { statusBorder: 'border-green-500', icon: 'lucide:check', color: 'text-success-500' }
		: { statusBorder: 'border-red-500', icon: 'lucide:x', color: 'text-error-500' };
</script>

<div class={['border-1 rounded-md border-solid p-2 pb-4', statusBorder]}>
	<span class="">{label}</span>
	<Handle type="target" position={targetPosition} {isConnectable} />
	<Handle type="source" position={sourcePosition} {isConnectable} />
	<Icon class={['absolute bottom-1 right-1', color].join(' ')} {icon} />
</div>

<style>
	:global(.svelte-flow__node-actionNode) {
		font-size: 12px;
		border-radius: 4px;
	}
</style>
