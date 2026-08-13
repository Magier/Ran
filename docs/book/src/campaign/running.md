# Running Multi-Step Emulation

Multi-step emulation is the main workflow in `ran emulate`. Rather than invoking
individual techniques in isolation, you follow the same discovery-and-exploitation
loop an adversary would.

## The emulation loop

1. **Start with a foothold.** Either use `--godmode` to seed the graph from your
   kubeconfig, or begin from a single compromised pod:

   ```sh
   ran emulate --target default/compromised-pod
   ```

2. **Select a target.** Click an entity in the cluster map. The armory panel
   updates to show only techniques applicable to that entity.

3. **Pick a technique.** Choose from the applicable TTPs. Read the description and
   note which procedures are available. Select the procedure that matches your
   current tooling (e.g. `kubectl` vs raw `curl` vs the built-in k8s-request).

4. **Choose K8S_AUTH and adjust parameters.** Kubernetes actions declare a
   `K8sAuth` parameter populated with eligible captured ServiceAccounts and the
   active K8sCredential. Other parameters retain sensible defaults derived from
   the target entity.

5. **Execute.** The output streams to the right panel. Watch the cluster map —
   newly discovered entities appear as nodes, and new relations appear as edges.

6. **Follow the graph.** The newly discovered entities are now available as targets.
   Select them and repeat from step 3.

7. **Clean up when done.** Use the Clean Up button for any technique that modified
   cluster state.

## Reading the cluster map

Entities in the map are coloured by type:

- **Blue** — Pods
- **Purple** — ServiceAccounts
- **Green** — Nodes
- **Yellow** — Roles and RoleBindings
- **Red** — C2 Servers and active sessions

Relation edges show the direction and type of the connection. Hover over an edge
to see its type. Edges with envelopes (escape paths, RCE chains) are drawn with a
dashed line.

## Following execution paths

Once a lateral movement path is established — e.g. a `container.escape` relation
from a pod to a node — you can select the node as a target and run techniques
directly on it. Ran routes the commands through the escape envelope automatically.

## Exporting the session

At any point you can export the current campaign as a MITRE Attack Flow document.
See [Reading the Attack Trail](trail.md).
