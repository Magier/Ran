<script lang="ts">
	import { Handle, Position, type NodeProps } from '@xyflow/svelte';
	import { iconMap } from '$lib/tactic_icons';
	import Icon from '@iconify/svelte';
	import type { AttackStep } from '$lib/api';
	import {getCampaignState, type Entity} from '$lib/components/CampaignState.svelte';

	const campaignState = getCampaignState();

	interface ActionNodeData extends Record<string, unknown> {
		step: AttackStep;
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

	const label = $derived(data.label);
	const step = $derived(data.step);
	const target: Entity = $derived.by(() => {
		const id = (step?.targetId || step?.executedOn);
		const e = campaignState.getEntityById(id) ?? { id: "?", name: 'Unknown' };
		return e
	});

	const statusMap = {
		Success: { statusBorder: 'border-green-500', icon: 'lucide:check', color: 'text-success-500' },
		Failed: { statusBorder: 'border-red-500', icon: 'lucide:x', color: 'text-error-500' },
		Ongoing: { statusBorder: 'border-yellow-500', icon: 'lucide:loader', color: 'text-warning-500' },
		Unknown: { statusBorder: 'border-surface-500', icon: 'lucide:help-circle', color: 'text-surface-500' },
	};
	const statusInfo = $derived(statusMap[step?.status] ?? statusMap.Unknown);
</script>

<div class={['bg-surface-50-950 border-1 rounded-md border-solid px-2 py-2', statusInfo.statusBorder]}>
		<span class="text-base">{label}</span>
		<div class="flex items-center space-x-1">
		<Icon icon={"game-icons:bullseye"} width="16" />
		<pre class="max-w-[200px] overflow-hidden text-ellipsis whitespace-nowrap" title={target.name ?? 'no target'}>{target.name ?? 'no target'}</pre>
		</div>
		<div class="w-full flex items-left mt-2">
		<div class="flex">
			<span class="badge bg-surface-100-900 text-tertiary-contrast-200-800">
				<Icon icon={iconMap[step.TTP.tactic]} width="16"/>
				{step.TTP.tactic}
			</span>
		</div>
		<div class="flex-1"></div>
		<div class="flex items-center space-x-1">
			{#if step.observables?.length > 0}
				<Icon icon={"humbleicons:eye"} width="16" />
			{/if}
			{#if step?.status && step.status !== 'Unknown'}
					<Icon class={statusInfo.color} icon={statusInfo.icon} width="16" />
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
