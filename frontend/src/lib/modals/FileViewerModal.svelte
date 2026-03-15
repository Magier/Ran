<script lang="ts">
	interface FileViewerProps {
		path: string;
		content: string;
		onClose: () => void;
	}

	let { path, content, onClose }: FileViewerProps = $props();
</script>

<div class="space-y-4 max-w-3xl">
	<header class="flex items-center justify-between">
		<h3 class="font-bold text-lg truncate" title={path}>{path}</h3>
		<button class="btn preset-outlined-surface-500 btn-sm" onclick={onClose}>✕</button>
	</header>
	<pre class="max-h-[70vh] overflow-auto rounded-md bg-surface-200-800 p-4 text-sm whitespace-pre-wrap break-words">{content}</pre>
	<footer class="flex justify-end">
		<button class="btn preset-filled-surface-500" onclick={onClose}>Close</button>
	</footer>
</div>
