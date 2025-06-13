<script lang="ts">
	import { Tree } from 'melt/builders';
	import { mergeAttrs } from 'melt';
	import Self from './tree.svelte'; // import itself for recursive rendering

	type Icon = 'svelte' | 'folder' | 'js';
	type TreeNode = {
		id: string;
		isOpen?: boolean;
		// icon: Icon;
		children?: TreeNode[];
	};

	// const icons = {
	// 	svelte: Svelte,
	// 	folder: Folder,
	// 	folderOpen: FolderOpen,
	// };
	type TreeProps = {
		entries?: any[];
		node?: TreeNode;
		level: number;
	};

	function getNodeName(node: TreeNode): string {
		if (node.name) {
			return node.name;
		}
		return `${node.mountPath} => ${node.hostPath}`;
	}

	let { entries = [], level = 0, node }: TreeProps = $props();

	let items = $derived.by(() => {
		if (node !== undefined) {
			return [node]; // use itself as the root node
		}
		return buildTree(entries);
	});

	function buildTree(data: TreeNode[]): TreeNode[] {
		const idMap: Record<string, TreeNode> = {};
		const rootNodes: TreeNode[] = [];

		// index all nodes by ID
		data.forEach((node) => {
			let nodeId = node.id || node.name;
			idMap[nodeId] = {
				...node,
				children: []
			};
		});

		// build the hierarchical tree
		data.forEach((node) => {
			let id = node.id || node.name;
			const mappedNode = idMap[id];
			if (node.parentId && idMap[node.parentId]) {
				idMap[node.parentId].children!.push(mappedNode);
			} else {
				rootNodes.push(mappedNode);
			}
		});

		return rootNodes;
	}

	let tree = new Tree({
		items: () => items,
		expandOnClick: true
	});
</script>

<div {...tree.root}>
	{#each tree.items as item}
		<div {...mergeAttrs(item.root, { class: `tree-item ${level > 0 ? 'ml-2' : ''}` })}>
			{#if item.children && item.children.length > 0}
				<button {...item.trigger} class="flex" onclick={() => tree.toggleExpand(item.id)}>
					<span class="mx-2">{tree.isExpanded(item.id) ? '-' : '+'}</span>
					<pre>{getNodeName(item)}</pre>
				</button>
			{:else}
				<pre class="ml-6">{getNodeName(item)}</pre>
			{/if}
			{#if item.children && tree.isExpanded(item.id)}
				<div {...item.content}>
					{#each item.children as child}
						<Self node={child} level={level + 1} />
					{/each}
				</div>
			{/if}
		</div>
	{/each}
</div>
