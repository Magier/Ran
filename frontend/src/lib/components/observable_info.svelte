<script lang="ts">
	interface Observable {
		title: string;
		type: string
	}
    interface ObservableInfoProps {
        observable: Observable;
    }

    let { observable }: ObservableInfoProps = $props();


	function prettyPrint(obj: any): string {
		if (typeof obj === 'string') {
			return obj;
			// } else if (typeof obj === 'object') {
			// 	if (Array.isArray(obj)) {
			// 		return obj.map((item) => prettyPrint(item)).join(', ');
			// 	} else if (obj === null) {
			// 		return 'null';
			// 	} else {
			// 		return JSON.stringify(obj, null, 2);
			// 	}
			// } else if (typeof obj === 'number') {
			// 	return obj.toString();
			// } else if (typeof obj === 'boolean') {
			// 	return obj ? 'true' : 'false';
		} else {
			return JSON.stringify(obj, null, 2);
		}
	}
</script>



{#each Object.entries(observable || {}) as [label, data]}
<div class="bg-surface-50-950 ">
    <div><span class="font-bold mr-1">{label}:</span>{prettyPrint(data)}</div>
</div>
{/each}