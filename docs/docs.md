# Design notes

Ran distinguishes two kinds of capability:

- `accessLevel` describes the adversary's access to an execution system.
- entitlements describe operations allowed by an authorization system such as Kubernetes RBAC.

Open modelling questions include session escalation across system boundaries, parser composition, optional procedure variants, and planner-friendly representations.

## Planning

The current deterministic input is Ran's YAML plan format. Longer-term research may include hierarchical tasks, behavior trees, classical planning, goal-oriented planning, reinforcement learning, and active inference.

MITRE Attack Flow/STIX import and export are future integration work; the current plan and campaign-flow formats are Ran-native YAML and JSON.
