<!-- Generated from armory/vocabulary.json. Do not edit directly. -->

# Armory Vocabulary

The machine-readable source is `armory/vocabulary.json`. Regenerate this page with `cargo run -p armory --bin generate-vocabulary-docs`.

Live Ran instances serve the same versioned document at `GET /api/armory/vocabulary`.

Vocabulary schema version: `2`. Stability: **experimental**.

This vocabulary describes declarations shipped with Ran. Custom Armory content may use additional names, but unknown procedure fields have no built-in semantics, unknown requirements do not gate applicability, and unknown effects have no built-in semantics unless an external parser handles them.

## Interpolation

Titles are rendered with execution arguments when available and may declare a human-readable fallback for Armory views. Before effects are processed, Ran substitutes execution arguments into every effect string. Effect argument lookup is ASCII case-insensitive.

Syntax: `${NAME} or ${NAME || fallback}`. Applies to: title, effects. Unknown variables: preserved unchanged.

## Support levels

| Level | Meaning |
| --- | --- |
| `enforced` | The declaration participates in applicability, readiness, or execution validation. |
| `parsed` | The effect has a built-in output parser. |
| `structural` | The effect directly changes campaign facts or graph structure. |
| `event` | The effect is confirmed through a runtime event rather than stdout. |
| `mixed` | Only the documented syntax variants have built-in semantics. |
| `declarative-only` | The declaration does not gate applicability or produce campaign facts. It may still be classified for scoring. |

## Procedure fields

Procedure fields describe how each execution alternative runs. Tool readiness is target-aware and is reported through `actionState.procedures`. Both `ready` and `unknown` procedures are runnable. Only `unavailable`, which means explicit absence is known, excludes a procedure. The action remains available while any procedure is `ready` or `unknown`.

| Name | Accepted value types | Required | Support | Scope | Meaning |
| --- | --- | --- | --- | --- | --- |
| `procedure.tool` | `string` | no | `enforced` | physical execution system | Names the binary dependency for one procedure. Procedure readiness is evaluated independently, so an action remains runnable while any alternative procedure is present or has unknown availability. A known-absent tool makes only that procedure unavailable. An explicit tool value wins. When tool is omitted, the YAML key is normalized into the tool field. For procedures constructed without either value, Ran falls back to a bare procedure ID and then the first command word. |

## Requirements

All requirement predicates must pass for a TTP to be applicable. Entries marked `declarative-only` are preserved in the API but do not currently gate applicability.

| Name | Accepted value types | Support | Matching semantics |
| --- | --- | --- | --- |
| `kind` | `string`, `array` | `enforced` | Restricts the semantic target kind. A string or any string array member matches the selected entity kind case-insensitively. System also matches any system entity. |
| `rbacPermissions` (YAML alias: `rbac`) | `array` | `enforced` | Requires Kubernetes RBAC permissions. The rbac YAML spelling is normalized before API serialization. One captured or active Kubernetes identity must satisfy every listed permission. An empty array passes. |
| `accessLevel` | `string` | `enforced` | Requires execution access to a system target. none passes. Any other string requires executable access, except for exempt tactics. |
| `activeSession` | `boolean` | `enforced` | Requires a live shell session. true requires an active session on the selected target. false imposes no restriction. |
| `filesystemAccess` | `boolean` | `enforced` | Requires a realizable path to the target filesystem. true requires direct filesystem execution access or an executable same-node Pod with a hostPath. false imposes no restriction. |
| `has-token` | `boolean` | `enforced` | Requires captured token material. true requires a non-empty token on the selected target. false imposes no restriction. |
| `exists` | `array` | `enforced` | Requires campaign entities to exist. Every entry must match an existing entity. Entries may be kind strings or objects with kind and optional exact name and namespace fields. |
| `related` | `array` | `enforced` | Requires entities related to the selected target. Every recognized target-kind and related-kind pair must be satisfied. Unknown relationship combinations pass. |
| `c2.has-listener` | `boolean` | `enforced` | Requires or forbids a registered C2 listener. The presence of any listener must equal the declared boolean. |
| `c2.has-session` | `boolean` | `enforced` | Requires or forbids a live C2 session. The selected C2 must own a live, unbroken session exactly when true is declared. |
| `c2.has-tool` | `string`, `array` | `enforced` | Requires operator-host executables. Every named executable must resolve on the operator host. Invalid array members are ignored. |
| `Pod.securityContext.privileged` | `boolean` | `enforced` | Matches the selected Pod privileged state. Known opposite evidence blocks the action. Unknown security context passes. |
| `Pod.securityContext.hostPID` | `boolean` | `enforced` | Matches the selected Pod host PID state. Known opposite evidence blocks the action. Unknown security context passes. |
| `Pod.hostPath` | `boolean`, `string`, `array` | `enforced` | Requires hostPath exposure on the selected Pod. A boolean requires or forbids any hostPath. A string requires one exact normalized source path. An array requires every listed source path. |
| `linuxNamespaceAccess` | `array` | `enforced` | Requires access to named Linux namespaces. Unknown access passes. The latest matching action evidence blocks a namespace after a recognized denial and restores it after success. |
| `sys.has-binary` | `string` | `declarative-only` | Legacy declaration of a target-side binary dependency. Procedures should declare their tool instead. No applicability predicate currently reads this requirement. |
| `Container.securityContext.capabilities` | `string`, `array` | `declarative-only` | Declares required Linux capabilities. No applicability predicate currently reads this requirement. |
| `pkg:golang/k8s.io/ingress-nginx` | `array` | `declarative-only` | Declares vulnerable ingress-nginx version ranges. No applicability predicate currently reads this package-specific requirement. |

## Effects

Effect kinds are the part before the first `(`. Effect matching is ASCII case-insensitive. Every effect string is interpolated before processing.

| Kind | Syntax | Support | Processing | Meaning |
| --- | --- | --- | --- | --- |
| `$c.name` | `$c.name(${NAME})` | `declarative-only` | none | Legacy container-name assertion. |
| `$d isa k8s.Deployment` | `$d isa k8s.Deployment(${TARGET})` | `declarative-only` | none | Legacy deployment assertion. |
| `$d.hasPod` | `$d.hasPod($p)` | `declarative-only` | none | Legacy deployment-to-Pod assertion. |
| `$p isa k8s.Pod` | `$p isa k8s.Pod(${TARGET})` | `declarative-only` | none | Legacy Pod assertion. |
| `$p.hasContainer` | `$p.hasContainer($c)` | `declarative-only` | none | Legacy Pod-to-container assertion. |
| `GCP.Buckets` | `GCP.Buckets` | `parsed` | stdout JSON | Discovers GCP bucket entities. |
| `GCP.projectID` | `GCP.projectID` | `declarative-only` | none | Declares discovery of a GCP project ID. |
| `GCP.serviceAccountToken` | `GCP.serviceAccountToken` | `declarative-only` | none | Declares discovery of a GCP service-account token. |
| `Namespace` | `Namespace($ns)` | `mixed` | deploy-container output parser | The literal Namespace($ns) form contributes namespace facts for Deploy Container. |
| `Namespace.name` | `Namespace.name` | `declarative-only` | scoring taxonomy only | Declares discovery of a namespace name. |
| `Pod.name` | `Pod.name` | `declarative-only` | scoring taxonomy only | Declares discovery of a Pod name. |
| `ServiceAccount.name` | `ServiceAccount.name` | `declarative-only` | scoring taxonomy only | Declares discovery of a ServiceAccount name. |
| `c2.listen` | `c2.listen(${PORT}, ${PROTOCOL})` | `event` | listener-started event | Confirms that a C2 listener was registered. |
| `c2.port-forward` | `c2.port-forward(${PLAY_ID}, ${RPORT}, ${LISTENER})` | `event` | redirector-started event | Confirms that a redirector was registered. |
| `c2.session` | `c2.session(<backend>, <target>)` | `structural` | graph relation | Records a live execution session from a C2 backend to a target. |
| `c2.stop-listener` | `c2.stop-listener(${ListenerID})` | `event` | listener-stopped event | Confirms that a C2 listener was removed. |
| `c2.stop-port-forward` | `c2.stop-port-forward(${RedirectorID})` | `event` | redirector-stopped event | Confirms that a redirector was removed. |
| `can-reach` | `can-reach(<target>)` | `declarative-only` | none | Legacy network-reachability declaration. |
| `container.escape` | `container.escape(<source>)` | `structural` | graph entities and relations | Records a container-to-node escape path. |
| `create k8s.CronJob` | `create k8s.CronJob` | `structural` | argument-derived custom resource | Creates a CronJob custom-resource entity from action arguments. |
| `create k8s.Pod` | `create k8s.Pod` | `parsed` | deploy-container arguments and events | Creates the deployed Pod and its immediately known facts. |
| `create k8s.Role` | `create k8s.Role` | `declarative-only` | scoring taxonomy only | Declares creation of a Kubernetes Role. |
| `create k8s.RoleBinding` | `create k8s.RoleBinding` | `declarative-only` | scoring taxonomy only | Declares creation of a Kubernetes RoleBinding. |
| `create k8s.ServiceMonitor` | `create k8s.ServiceMonitor` | `structural` | argument-derived custom resource | Creates a ServiceMonitor custom-resource entity from action arguments. |
| `created` | `created(creator:$p1, target:$p2)` | `parsed` | deploy-container output parser | Marks the Deploy Container result as action-created. |
| `delete k8s.Pod` | `delete k8s.Pod` | `structural` | target removal | Removes the target Pod after successful execution. |
| `delete k8s.ServiceAccount` | `delete k8s.ServiceAccount` | `structural` | target removal | Removes the target ServiceAccount after successful execution. |
| `delete k8s.deployment` | `delete k8s.deployment` | `declarative-only` | none | Declares deletion of a Deployment. |
| `file:content` | `file:content(${PATH})` | `parsed` | stdout content parser | Captures file contents and dispatches content-specific parsing. |
| `file:kubeconfig` | `file:kubeconfig` | `parsed` | stdout kubeconfig parser | Creates Kubernetes credentials from kubeconfig content. |
| `k8s.Namespace.enforcedPSS=privileged` | `k8s.Namespace.enforcedPSS=privileged` | `declarative-only` | none | Declares a namespace Pod Security Standards change. |
| `k8s.Role` | `k8s.Role` | `structural` | action arguments | Creates a Kubernetes Role entity from action arguments. |
| `k8s.SelfSubjectRulesReview` | `k8s.SelfSubjectRulesReview` | `parsed` | stdout JSON | Records effective Kubernetes permissions for an identity. |
| `k8s.ServiceAccount` | `k8s.ServiceAccount` | `structural` | action arguments | Creates a ServiceAccount entity from action arguments. |
| `k8s.can-exec` | `k8s.can-exec(<source>, <target>)` | `structural` | graph relation | Records Kubernetes exec capability between entities. |
| `k8s.clusterRoleBindingList` | `k8s.clusterRoleBindingList` | `parsed` | stdout Kubernetes JSON | Discovers ClusterRoleBinding entities. |
| `k8s.clusterRoleList` | `k8s.clusterRoleList` | `parsed` | stdout Kubernetes JSON | Discovers ClusterRole entities. |
| `k8s.configmaplist` | `k8s.configmaplist` | `parsed` | stdout Kubernetes JSON | Discovers ConfigMap entities. |
| `k8s.deploymentList` | `k8s.deploymentList` | `parsed` | stdout Kubernetes JSON | Discovers Deployment entities. |
| `k8s.gatewaylist` | `k8s.gatewaylist` | `parsed` | stdout Kubernetes JSON | Discovers Gateway API Gateway entities. |
| `k8s.httproutelist` | `k8s.httproutelist` | `parsed` | stdout Kubernetes JSON | Discovers Gateway API HTTPRoute entities. |
| `k8s.ingresslist` | `k8s.ingresslist` | `parsed` | stdout Kubernetes JSON | Discovers Ingress entities. |
| `k8s.kubelet-exec` | `k8s.kubelet-exec(<source>, <target-or-all>)` | `structural` | graph relation | Records kubelet-mediated execution capability. |
| `k8s.namespaceList` | `k8s.namespaceList` | `parsed` | stdout Kubernetes JSON | Discovers Namespace entities. |
| `k8s.nodeList` | `k8s.nodeList` | `parsed` | stdout Kubernetes JSON | Discovers Node entities. |
| `k8s.pod.IsRunning=false` | `k8s.pod.IsRunning=false` | `declarative-only` | none | Declares a non-running Pod outcome. |
| `k8s.podList` | `k8s.podList` | `parsed` | stdout Kubernetes JSON | Discovers Pod entities. |
| `k8s.roleBindingList` | `k8s.roleBindingList` | `parsed` | stdout Kubernetes JSON | Discovers RoleBinding entities. |
| `k8s.roleList` | `k8s.roleList` | `parsed` | stdout Kubernetes JSON | Discovers Role entities. |
| `k8s.secretList` | `k8s.secretList` | `parsed` | stdout Kubernetes JSON | Discovers Secret metadata. |
| `k8s.serviceAccountList` | `k8s.serviceAccountList` | `parsed` | stdout Kubernetes JSON | Discovers ServiceAccount entities. |
| `k8s.servicelist` | `k8s.servicelist` | `parsed` | stdout Kubernetes JSON | Discovers Service entities and reachability facts. |
| `linux.mounts` | `linux.mounts` | `parsed` | stdout mount parser | Records mounted filesystems on the execution system. |
| `network.discovery` | `network.discovery` | `parsed` | stdout network discovery parser | Discovers reachable hosts and services from nmap or reverse-DNS output. |
| `ns.contains` | `ns.contains($p2)` | `mixed` | deploy-container output parser | Only the literal ns.contains($p2) form contributes facts for Deploy Container; ns.contains($p) is declarative-only. |
| `rawServiceaccountToken` | `rawServiceaccountToken` | `parsed` | stdout JWT parser | Validates and records captured Kubernetes service-account tokens. |
| `rce.can-exec` | `rce.can-exec(<source>, <target>)` | `structural` | graph relation | Records remote-code-execution capability between entities. |
| `sys.envVar` | `sys.envVar` | `parsed` | stdout environment parser | Records environment variables on the execution system. |
| `sys.files` | `sys.files` | `parsed` | stdout file-list parser | Records files and directories on the execution system. |
| `sys.has-binary` | `sys.has-binary(<name-or-path>[, <output-source>])` | `parsed` | arguments and optional stdout | Records an executable as present on the execution system. |
| `sys.hasBinary` | `sys.hasBinary` | `declarative-only` | scoring taxonomy only | Legacy binary-presence declaration without an argument. |
| `sys.hasFile` | `sys.hasFile` | `declarative-only` | scoring taxonomy only | Legacy file-presence declaration without a path argument. |
| `sys.ip` | `sys.ip` | `parsed` | stdout IP parser | Records IP addresses on the execution system. |
| `sys.processes` | `sys.processes` | `parsed` | stdout process-list parser | Records processes on the execution system. |
| `sys.userID` | `sys.userID` | `parsed` | stdout identity parser | Records the user ID and access level on the execution system. |
