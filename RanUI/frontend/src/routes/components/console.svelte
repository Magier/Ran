<script lang="ts">
	import store from '$lib/stores/store';

	type HistoryEntry = {
		command: string,
		result: string,
		timestamp: Date
	}
	
	let history: HistoryEntry[] = [];

	function onPromptKeydown(event: KeyboardEvent): void {
		if (['Enter'].includes(event.code)) {
			event.preventDefault();
			sendCommand();
		}
	}

	function sendCommand() {
		history = [...history, {command: cmd, result: "result", timestamp: new Date()}]
		store.sendMessage("terminal", {data: cmd})
		cmd = "";
	};

	let cmd: string;

</script>

<div class="h-full grid grid-rows-[1fr_auto] gap-1">
	<div class="bg-surface-500/30 p-4">Console</div>
	<div class="bg-surface-500/30 p-4 overflow-y-auto">

	{#each history as record }
	<div class="grid grid-cols-[auto_1fr] gap-2">
		<div class="card p-4 variant-soft rounded-tl-none space-y-2">
			<header class="flex justify-between items-center">
				<p class="font-bold">{record.command}</p>
				<small class="opacity-50">{record.timestamp.toLocaleString('en-US', { hour: 'numeric', minute: 'numeric', hour12: true })}</small>
			</header>
			<p>{record.result}</p>
		</div>
	</div>
	{/each}</div>
	<!-- <div class="bg-surface-500/30 p-4">
		</div>	 -->
			<div class="input-group input-group-divider grid-cols-[auto_1fr_auto] rounded-container-token">
				<div class="input-group-shim">>_</div>
				<input type="search" placeholder="Enter command ..." bind:value={cmd} on:keydown={onPromptKeydown} />
				<button class="variant-filled-secondary" on:click={sendCommand}>Send</button>
		</div>
	</div>
