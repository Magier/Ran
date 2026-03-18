<script lang="ts">
	import type { RBACPermission } from '$lib/api';

    type EntitlementsInfoProps = {
        entitlements: RBACPermission[];
        getUtility: (e: RBACPermission) => number;
    };

    let { entitlements, getUtility }: EntitlementsInfoProps = $props();

    function getStyle(e: RBACPermission): string {
        const utility = getUtility(e);
        if (utility >= 10) {
            return 'text-error-700-300 font-bold';
        }else if (utility > 5) {
            return 'text-success-800-200';
        } else if (utility > 0) {
            return 'text-secondary-700-300';
        } else {
            return 'text-surface-500';
        }
    }

</script>

<pre class="max-h-80 overflow-scroll flex flex-col gap-1 pl-5 ">
    {#each entitlements as e}
        <span class={getStyle(e)}>{e.verb} {e.resourceType}{e.resourceName}{#if e.scope} in {e.scope} {/if}{#if e.apiGroup}({e.apiGroup}){/if}</span>
    {/each}
</pre>