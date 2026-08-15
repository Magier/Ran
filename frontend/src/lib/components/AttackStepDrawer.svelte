<script lang="ts">
	import type { AttackStep } from '$lib/api';
	import { Dialog, Portal } from '@skeletonlabs/skeleton-svelte';
	import AttackStepDetails from './attack_step_details.svelte';

	interface Props {
		step: AttackStep | null;
		onclose: () => void;
	}

	let { step, onclose }: Props = $props();

	const animDrawer =
		'transition transition-discrete opacity-0 translate-x-full starting:data-[state=open]:opacity-0 starting:data-[state=open]:translate-x-full data-[state=open]:opacity-100 data-[state=open]:translate-x-0';
	const animBackdrop =
		'transition transition-discrete opacity-0 starting:data-[state=open]:opacity-0 data-[state=open]:opacity-100';
</script>

<Dialog
	open={step !== null}
	onOpenChange={(event) => {
		if (!event.open) onclose();
	}}
>
	<Portal>
		<Dialog.Backdrop
			class="fixed inset-0 z-50 bg-surface-50-950/50 transition transition-discrete {animBackdrop}"
		/>
		<Dialog.Positioner class="fixed inset-0 z-50 flex justify-end">
			<Dialog.Content
				class="h-screen w-xl space-y-4 overflow-auto bg-surface-100-900 p-4 shadow-xl {animDrawer}"
			>
				{#if step}
					<AttackStepDetails {step} />
				{/if}
			</Dialog.Content>
		</Dialog.Positioner>
	</Portal>
</Dialog>
