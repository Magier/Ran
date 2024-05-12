<script lang="ts">
	import type { SvelteComponent } from 'svelte';
	import { getModalStore } from '@skeletonlabs/skeleton';
	// Props
	/** Exposes parent props to this component. */
	export let parent: SvelteComponent;
	const modalStore = getModalStore();

	// Base Classes
	const cBase = 'card p-4 w-modal shadow-xl space-y-4';
	const cHeader = 'text-2xl font-bold';
	const cForm = 'border border-surface-500 p-4 space-y-4 rounded-container-token';

	interface PodParams {
		name: string;
		image: string;
		cmd?: string;
		args?: string[],
		useHostIPC?: boolean;
		useHostPID?: boolean;
		useHostNetwork?: boolean;
	}

	let podParams: PodParams = {
		name: $modalStore[0].valueAttr.name,
		image: $modalStore[0].valueAttr.image,
		cmd: $modalStore[0].valueAttr.cmd,
		args: $modalStore[0].valueAttr.args,
		useHostIPC: $modalStore[0].valueAttr.hostIPC ?? false,
		useHostPID: $modalStore[0].valueAttr.hostIPC ?? false,
		useHostNetwork: $modalStore[0].valueAttr.hostIPC ?? false
	};

	// We've created a custom submit function to pass the response and close the modal.
	function onFormSubmit(): void {
		if ($modalStore[0].response) $modalStore[0].response(podParams);
		modalStore.close();
	}
</script>

{#if $modalStore[0]}
	<div class="modal-example-form {cBase}">
		<header class={cHeader}>{$modalStore[0].title ?? '(title missing)'}</header>
		<article>
			{$modalStore[0].body ?? '`$C2` is a variable for the listener of reverse shells'}
		</article>
		<form class="modal-form {cForm}">
			<label class="label">
				<span>Name</span>
				<input
					class="input"
					type="text"
					bind:value={podParams.name}
					placeholder="Name of the Pod"
				/>
			</label>
			<label class="label">
				<span>Image</span>
				<input
					class="input"
					type="text"
					bind:value={podParams.image}
					placeholder="Image of the container"
				/>
			</label>

			<label class="label">
				<span>Command</span>
				<input class="input" type="text" bind:value={podParams.cmd} placeholder="Start command" />
			</label>
			<label class="label">
				<span>Args</span>
				<input class="input" type="text" bind:value={podParams.args} placeholder="Start command" />
			</label>
			<div class="space-y-2">
				<label class="flex items-center space-x-2">
					<input class="checkbox" type="checkbox" bind:checked={podParams.useHostIPC} />
					<p>Use HostIPC</p>
				</label>
				<label class="flex items-center space-x-2">
					<input class="checkbox" type="checkbox" bind:checked={podParams.useHostPID} />
					<p>Use HostPID</p>
				</label>
				<label class="flex items-center space-x-2">
					<input class="checkbox" type="checkbox" bind:checked={podParams.useHostNetwork} />
					<p>Use HostNetwork</p>
				</label>
			</div>
		</form>
		<footer class="modal-footer {parent.regionFooter}">
			<button class="btn {parent.buttonNeutral}" on:click={parent.onClose}
				>{parent.buttonTextCancel}</button
			>
			<button class="btn {parent.buttonPositive}" on:click={onFormSubmit}>Deploy</button>
		</footer>
	</div>
{/if}
