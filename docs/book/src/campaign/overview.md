# What is a Campaign?

Running a single TTP is useful for verifying one detection rule. But real attackers
don't fire isolated commands — they explore, adapt, and chain techniques based on
what they discover. A **campaign** is Ran's model for that kind of continuous,
evolving emulation.

## The core idea

When you start `ran emulate`, Ran creates a live **knowledge graph** of your cluster.
The graph starts empty (or pre-seeded if you use `--godmode`). As you execute
techniques, the graph grows:

1. You run *Read Environment Variables* on a compromised pod.
2. Ran parses the output and discovers a `ServiceAccount` token in the environment.
3. The new service account appears in the graph.
4. Ran checks which techniques are now applicable — and surfaces *Get Pods*, *Get
   Secrets*, and any other technique that requires a captured service account.
5. You pick *Create Admin Role*, which requires RBAC `create` on `roles`.
6. Ran checks whether the captured service account has that permission. It does.
7. The technique runs. A new `K8sRole` entity appears in the graph, bound to the
   service account via a `k8s.rolebinding` relation.

At each step the graph reflects the actual state of your foothold — not a static
inventory, but a record of what an adversary would know and be able to do at that
moment.

## What a campaign tracks

- **Entities** — discrete cluster resources: pods, nodes, service accounts, roles,
  role bindings, C2 servers, and more. See [The Context Model](context-model.md).
- **Relations** — how entities connect: which pod runs on which node, which
  service account can exec into which pod, which node has been escaped from.
- **Execution record** — the ordered log of every TTP invocation, its parameters,
  its raw output, and which entities and relations it produced.

## Campaign lifecycle

A campaign exists for the duration of a `ran emulate` session. It is not persisted
to disk by default. The execution record can be exported as a MITRE Attack Flow
document for reporting and follow-up analysis — see [Reading the Attack Trail](trail.md).
