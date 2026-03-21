<script lang="ts">
	import type { RBACPermission } from '$lib/api';

    type EntitlementsInfoProps = {
        entitlements: RBACPermission[];
    };

    let { entitlements }: EntitlementsInfoProps = $props();

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


	function getUtility(e: RBACPermission): number {
		let utility = 0;

		// regular API endpoints have no real value (for now?)
		if (e.resourceName && e.resourceName.startsWith('/') && !e.resourceType) {
			return 0;
		}

		if (e.verb === 'get' || e.verb === 'list') {
			utility += 1;
		} else if (e.verb === 'create' || e.verb === 'update' || e.verb === 'patch') {
			utility += 3;
		} else if (e.verb === 'delete') {
			utility += 2;
		} else if (e.verb === '*') {
			utility += 10;
		}

		if (e.resourceType) {
			if (e.resourceType.startsWith('pod') || e.resourceType === 'deployments') {
				utility += 5;
			} else if (e.resourceType.includes('*')) {
				utility += 10;
			} else if (e.resourceType == 'nodes/proxy') {
				utility += 9;
			} else if (e.resourceType.startsWith('nodes')) {
				utility += 7;
			} else if (e.resourceType === 'secrets' || e.resourceType === 'configmaps') {
				utility += 8;
			} else if (e.resourceType === 'namespaces') {
				utility += 6;
			} else if (e.resourceType.includes('role')) {
				utility += 8;
            } else if (e.resourceType.startsWith('serviceaccount')) {
				utility += 4;
			} else if (e.resourceType.startsWith('selfsubject')) {
				return 0;
			}
		}
		return utility;
	}

	function getSortedEntitlements(entitlements: RBACPermission[]): RBACPermission[] {
		return [...entitlements].sort((a, b) => getUtility(b) - getUtility(a));
	}

</script>

<pre class="max-h-80 overflow-scroll flex flex-col gap-1 pl-5 ">
    {#each getSortedEntitlements(entitlements) as e}
        <span class={getStyle(e)}>{e.verb} {e.resourceType}{e.resourceName}{#if e.scope} in {e.scope} {/if}{#if e.apiGroup}({e.apiGroup}){/if}</span>
    {/each}
</pre>