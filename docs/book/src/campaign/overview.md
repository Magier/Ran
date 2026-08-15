# What is a Campaign?

A campaign is Ran's live model of a multi-step emulation. It combines a knowledge graph with an ordered execution history.

As TTPs run, output parsers and effects add entities and relations. Those facts change which TTPs are applicable and which execution channels can reach a target. The browser UI shows this evolving state and lets the operator choose the next action.

A campaign tracks:

- entities such as pods, nodes, service accounts, credentials, and C2 systems;
- relations such as containment, credentials, permissions, and execution channels;
- execution records with the TTP, target, procedure, result, and derived facts.

Campaign state lives for the server session unless reset. Use **Save Ran JSON** or `GET /api/flow` to download the current flow. This native JSON is not MITRE Attack Flow/STIX.
