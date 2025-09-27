<script lang="ts">
	import {domain} from '$lib/wailsjs/go/models';

    type EntitlementsInfoProps = {
        entitlements: domain.RBACPermission[]
    };

    let { entitlements}: EntitlementsInfoProps = $props();

    function getUtility(e: domain.RBACPermission): number {
        let utility = 0;

        // regular API endpoints have no real value (for now?)
        if (e.resourceName && e.resourceName.startsWith('/') && !e.resourceType) {
            return 0;
        }

        if (e.verb === 'get' || e.verb === 'list') {
            utility += 1;
        } else if (e.verb === 'create' || e.verb === 'update' || e.verb === 'patch') {
            utility += 2;
        } else if (e.verb === 'delete') {
            utility += 2;
        } else if (e.verb === '*') {
            utility += 10;
        }

        if (e.resourceType) {
            if (e.resourceType.startsWith('pod') || e.resourceType === 'deployment') {
                utility += 5;
            } else if (e.resourceType === 'node' || e.resourceType === 'namespace') {
                utility += 6;
            } else if (e.resourceType === 'secret' || e.resourceType === 'configmap' || e.resourceType.startsWith('serviceaccount')) {
                utility += 8;
            } else if (e.resourceType.includes('role')) {
                utility += 8;
            } else if (e.resourceType.startsWith('selfsubject')) {
                return 1;
            }
        }
        return utility; // Unknown verb
    }

    function getStyle(e: domain.RBACPermission): string {
        const utility = getUtility(e);
        if (utility > 5) {
            return 'text-success-500';
        } else if (utility > 0) {
            return 'text-success-700';
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