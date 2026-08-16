# KubeTier escalation-path coverage

[KubeTier source](https://kubetier.com/escalation/) (snapshot: 2026-08-15,
validated by KubeTier against Kubernetes 1.36.1)

This table documents KubeTier escalation paths for which Ran already has an
executable TTP or an explicit disabled/partial implementation. Coverage means
Ran can perform the decisive Kubernetes or workload operation; environmental
preconditions described by KubeTier still apply. Discovery-only similarity is
not counted as support.

| KubeTier path | Tier | Ran implementation | Coverage | Notes |
|---|---:|---|:---:|---|
| [Privileged pod node escape](https://kubetier.com/escalation-privileged-pod) | T1 | [Deploy Container](<../armory/TTPs/Execution/deploy_container.yaml>) | ✅ | Supports privileged mode, host namespaces, node selection, and hostPath mounting. |
| [Bad pods host namespace abuse](https://kubetier.com/escalation-bad-pods) | T1 | [Deploy Container](<../armory/TTPs/Execution/deploy_container.yaml>) | ✅ | Exposes `HostPID`, `HostIPC`, and `HostNetwork` parameters. |
| [Pod ServiceAccount token theft](https://kubetier.com/escalation-pod-satoken-theft) | T1 | [Deploy Container and Mount SA Token](<../armory/TTPs/CredentialAccess/create_pod_using_the_serviceaccount.yaml>) | ✅ | Creates a pod under a selected ServiceAccount and reads its mounted token. |
| [Inject code into existing workloads](https://kubetier.com/escalation-workload-inject) | T1 | [Inject Sidecar into template](<../armory/TTPs/Execution/inject_sidecar_template.yaml>) | ✅ | Patches a Deployment pod template with an attacker-controlled container. |
| [Ephemeral container injection](https://kubetier.com/escalation-ephemeral-node-debug) | T1 | [Inject ephemeral Sidecar](<../armory/TTPs/Execution/inject_sidecar_ephemeral.yaml>) | ⚠️ | Procedure uses `kubectl debug`; its RBAC precondition still models `patch pods` rather than the precise ephemeral-container subresource. |
| [Exec into a privileged pod](https://kubetier.com/escalation-exec-privileged) | T1 | [Execute into pod via Valid Account](<../armory/TTPs/InitialAccess/valid_accounts_kubeconfig.yaml>) + [Execute in Shell](<../armory/TTPs/Execution/execute_shell.yaml>) | ✅ | Establishes a Kubernetes exec channel and then runs arbitrary commands; privilege depends on the selected pod. |
| [Cluster-wide secret harvesting](https://kubetier.com/escalation-secret-list-all) | T0 | [List K8s secrets](<../armory/TTPs/CredentialAccess/list_k8s_secrets.yaml>) | ✅ | `ALL_NS=true` lists Secrets across namespaces. |
| [Namespace secret credential theft](https://kubetier.com/escalation-secret-theft) | T1 | [List K8s secrets](<../armory/TTPs/CredentialAccess/list_k8s_secrets.yaml>) | ✅ | Lists and records Secret entities in a selected namespace. |
| [kube-system secret retrieval](https://kubetier.com/escalation-kubesystem-secret-get) | T1 | [List K8s secrets](<../armory/TTPs/CredentialAccess/list_k8s_secrets.yaml>) | ✅ | Supported when `kube-system` is selected and the authenticated identity has access. |
| [tokenRequest for privileged SA](https://kubetier.com/escalation-sa-token-create) | T1 | [Create New Service Account Token](<../armory/TTPs/CredentialAccess/create_sa_token.yaml>) | ✅ | Calls the ServiceAccount TokenRequest subresource for the selected account. |
| [Bypass PodDisruptionBudget by deleting pods](https://kubetier.com/escalation-pod-delete-pdb-bypass) | T2 | [Delete Pod](<../armory/TTPs/Impact/delete_pod.yaml>) | ✅ | Uses direct pod deletion, which bypasses the eviction API and its PDB check. |
| [Disable pod security admission](https://kubetier.com/escalation-disable-psa) | T1 | [Disable PSA on Namespace](<../armory/TTPs/Defense%20Impairment/disable_psa_on_namespace.yaml>) | ✅ | Overwrites the namespace enforcement label with `privileged`. |
| [hostNetwork NetworkPolicy bypass](https://kubetier.com/escalation-hostnetwork-networkpolicy-bypass) | T1 | [Deploy Container](<../armory/TTPs/Execution/deploy_container.yaml>) | ✅ | Creates a pod with `hostNetwork: true`. |
| [Node cloud identity reached past workload identity concealment](https://kubetier.com/escalation-node-identity-metadata) | T1 | [Deploy Container](<../armory/TTPs/Execution/deploy_container.yaml>) + [Get GCP Service Account Token](<../armory/TTPs/CredentialAccess/get_GCP_servicaccount_token.yaml>) | ⚠️ | Ran implements the host-network/hostPath setup and GCP metadata-token retrieval, but not the EKS/AKS variants. |
| [nodes/proxy WebSocket exec bypass](https://kubetier.com/escalation-nodes-proxy-exec) | T0 | [Execute via Node/Proxy](<../armory/TTPs/Execution/execute_node-proxy-exec.yaml>) | 🚧 | The procedure exists but is disabled and its `nodes/proxy` RBAC requirement is still commented out. |
| [externalIP service traffic interception](https://kubetier.com/escalation-externalip-intercept) | T1 | [IP/Service Spoofing (CVE-2020-8554)](<../armory/TTPs/Lateral%20Movement/mitm-cve-2020-8554.yaml>) | 🚧 | Creates a Service with attacker-selected `externalIPs`; the TTP is currently disabled. |

Legend: ✅ executable coverage · ⚠️ partial or imprecise coverage · 🚧 implementation present but disabled.
