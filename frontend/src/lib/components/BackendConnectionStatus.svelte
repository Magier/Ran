<script lang="ts">
	import { onDestroy, onMount } from 'svelte';
	import Icon from '@iconify/svelte';
	import { getRanAPI, type BackendConnectionState } from '$lib/ran_api';

	type VisibleStatus = 'disconnected' | 'reconnected' | null;

	const RECONNECTED_VISIBLE_MS = 4_000;
	let visibleStatus: VisibleStatus = $state(null);
	let wasDisconnected = false;
	let hideTimer: ReturnType<typeof setTimeout> | undefined;

	function clearHideTimer() {
		if (hideTimer) clearTimeout(hideTimer);
		hideTimer = undefined;
	}

	function handleConnectionState(state: BackendConnectionState) {
		if (state === 'disconnected') {
			clearHideTimer();
			wasDisconnected = true;
			visibleStatus = 'disconnected';
			return;
		}

		if (state === 'connected' && wasDisconnected) {
			visibleStatus = 'reconnected';
			clearHideTimer();
			hideTimer = setTimeout(() => {
				visibleStatus = null;
				wasDisconnected = false;
			}, RECONNECTED_VISIBLE_MS);
		}
	}

	let unsubscribe: (() => void) | undefined;
	onMount(() => {
		unsubscribe = getRanAPI().onConnectionStateChange(handleConnectionState);
	});

	onDestroy(() => {
		clearHideTimer();
		unsubscribe?.();
	});
</script>

{#if visibleStatus}
	<div
		class:status-disconnected={visibleStatus === 'disconnected'}
		class:status-reconnected={visibleStatus === 'reconnected'}
		class="connection-status fixed bottom-4 left-4 z-50 flex items-center gap-2 rounded-md px-3 py-2 text-sm shadow-lg"
		role="status"
		aria-live="polite"
	>
		<Icon
			icon={visibleStatus === 'disconnected' ? 'mdi:link-variant-off' : 'mdi:link-variant'}
			class="size-4"
			aria-hidden="true"
		/>
		{visibleStatus === 'disconnected'
			? 'Disconnected from backend. Reconnecting...'
			: 'Reconnected to backend'}
	</div>
{/if}

<style>
	.connection-status {
		color: var(--color-surface-contrast-950);
	}

	.status-disconnected {
		background: var(--color-error-500);
		color: var(--color-error-contrast-500);
	}

	.status-reconnected {
		background: var(--color-success-500);
		color: var(--color-success-contrast-500);
	}
</style>
