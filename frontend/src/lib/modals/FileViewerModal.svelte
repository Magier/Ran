<script lang="ts">
	interface FileViewerProps {
		path: string;
		content: string;
		onClose: () => void;
	}

	let { path, content, onClose }: FileViewerProps = $props();
</script>

<div class="max-w-3xl space-y-4">
	<header class="flex items-center justify-between">
		<h3 class="truncate text-lg font-bold" title={path}>{path}</h3>
		<button class="btn preset-outlined-surface-500 btn-sm" onclick={onClose}>✕</button>
	</header>
	<pre
		class="bg-surface-200-800 max-h-[70vh] overflow-auto rounded-md p-4 text-sm break-words whitespace-pre-wrap">{content}</pre>
	<footer class="flex justify-end">
		<button class="btn preset-filled-surface-500" onclick={onClose}>Close</button>
	</footer>
</div>
