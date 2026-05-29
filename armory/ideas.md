
## Initial Access
- Wiz [Making Sense of Kubernetes Initial Access Vectors Part 1 – Control Plane](https://www.wiz.io/blog/making-sense-of-kubernetes-initial-access-vectors-part-1-control-plane)
- Wiz [Making Sense of Kubernetes Initial Access Vectors Part 2 - Data Plane](https://www.wiz.io/blog/kubernetes-data-plane)

## Enumeration

https://github.com/beserkerbob/KubernetesEnumerationTool (Powershell)

- [Kubernetes DNS-Based Service Discovery](https://github.com/kubernetes/dns/blob/master/docs/specification.md)

- [Demystifying The First Few Minutes After Compromising A Container - Stuart McMurray](https://www.youtube.com/watch?v=j4757Q06ev8)


- support [Nerva](https://github.com/praetorian-inc/nerva) to understand service behind a port: Fast service fingerprinting CLI for 170+ protocols (TCP/UDP/SCTP) 

## Privilege Escalation
- SentinelOne [Climbing The Ladder | Kubernetes Privilege Escalation (Part 1)](https://www.sentinelone.com/blog/climbing-the-ladder-kubernetes-privilege-escalation-part-1)  
- SentinelOne [Climbing The Ladder | Kubernetes Privilege Escalation (Part 2)](https://www.sentinelone.com/blog/climbing-the-ladder-kubernetes-privilege-escalation-part-2/)

- [Sudo: Local Privilege Escalation via host option](https://www.sudo.ws/security/advisories/host_any/)
	- Sudo versions 1.8.8 to 1.9.17 inclusive are affected. (CVE-2025-32462)

- [CVE-2024-45310](https://github.com/advisories/GHSA-jfvp-7x6p-h2pv)
	- from talk ["containers / security / a fun time -- pick two" - Aleksa (purplecon 2024)](https://www.youtube.com/watch?v=cY4ko-KhDGU)


## Lateral Movement
- Wiz [NamespaceHound](https://github.com/wiz-sec-public/namespacehound)
- given: hostPath mount: put a static pod manifest on the node, which spawns a privileged pod
	- try with invalid namespace name in manifest -> visibile in k8s api?

- Workload that can `update` itself:
	- change to privileged -> escape to host
	- change service account
	- schedule to specific node 
	- ref: https://youtu.be/1rmg2QfLJtY?t=671
	- ![alt text](permissions_example_rsac_2023.png)


## Cloud Pivot:
-  direct IMDSv1 access
	- Azure: 169.254.169.254/metadata/identity/oauth2
	- AWS: 169.254.169.254/latest/meta-data/iam/security-credentials
	- GCP: metadata.google.internal/computeMetadata/v1/instance/service-accounts
- By default, the EC2 role has the policies:
	- AmazonEC2ContainerRegistryReadOnly - Pull permissions to the container registry.
	- AmazonEKSWorkerNodePolicy - Read permissions to the compute environment (EC2, VPC etc.)
	- AmazonEKS_CNI_Policy - Attach network interfaces and IPs to VMs

- OIDC: https://aws.amazon.com/blogs/containers/introducing-fine-grained-iam-roles-service-accounts/
	- GKE has unified identity pool in the project: https://youtu.be/1rmg2QfLJtY?t=1912
		- security boundary is the project (not the cluster) -> pivot between clusters?


## Discovery

- [K8spider](https://github.com/Esonhugh/k8spider): supports to scan all services installed in Kubernetes cluster and all exposed ports in service
- Use [RBAC-Atlas](https://rbac-atlas.github.io/) to quickly learn about the permissions of 3rd Party software in the cluster

- [CoreDNS Enum](https://github.com/jpts/coredns-enum)

- Talk from BSidesLV 2025 [From interview questions to cluster damage: Adventures in k8s cluster shenanigans](https://youtu.be/gPEnfkFM2Hw?t=29422) hit 3rd party observability tools for discovery
	- like kubecost metrics endpoint, that will report all pods and nodes in the cluster.
	- the 1 API request is nearly impossible to detect, but it will give you a full list of pods and nodes in the cluster.

## Interesting tools to try to support

### SSH
- [Dropbear](https://github.com/mkj/dropbear) 

### Tunnels
- [bore](https://github.com/ekzhang/bore)  
	- alternative to ngrok

### Rootkits

- [TripleCross](https://github.com/h3xduck/TripleCross) (2022)

### Sniffer

- [k8s-sniff-https](https://github.com/ofirc/k8s-sniff-https)

### LolBINS
- [GTFOBins](https://gtfobins.github.io): list of Unix binaries that can be used to bypass local security restrictions in misconfigured systems
- [LOTTunnels](https://lottunnels.github.io): Living Off The Land Tunnels
- [LOLC2](https://lolc2.github.io/)
- [LOLRMM](https://lolrmm.io/)

#### Tools for potential support
- https://github.com/vulsio/go-exploitdb
- [PEASS-ng](https://github.com/peass-ng/PEASS-ng)
- [go-pillage-registries](https://github.com/nccgroup/go-pillage-registries)
- [amicontained](https://github.com/genuinetools/amicontained)
- [dopwn](https://github.com/4ARMED/dopwn)
- [botb](https://github.com/brompwnie/botb)
- [MTKPI](https://github.com/r0binak/MTKPI) Multi Tool Kubernetes Pentest Image 
- [deepce](https://github.com/stealthcopter/deepce)
- [ctrsploit](https://github.com/ctrsploit/ctrsploit)


## Catalog of interesting TTPs

-[Unprotect Project](https://unprotect.it/map/)


## Vulnerabilities

### Vulns supported by ctrsploit 
- CVE-2020-15257 Abuse the containerd-shim's abstract unix socket when running in a container with host network namespace.
- CVE-2025-47290 TOCTOU vulnerability in containerd that allows modification of the host filesystem during image pull.



## Invert detection rules

- [Chainguard's OSquery-defense-kit](https://github.com/chainguard-dev/osquery-defense-kit/tree/main)
- [Elastic Deteciton Rules](https://github.com/elastic/detection-rules/tree/main/rules/linux)
- [Falco Rules](https://falcosecurity.github.io/rules/)

- [K8s Custom Detections](https://github.com/heilancoos/k8s-custom-detections/) repo with a collection of detections + attack scripts to test them

## LLM Planner

- useful LLM Action Constraints discussed in CyberLayer scenario in the talk [What Lies Beneath the Surface? Evaluating LLMs for Offensive Cyber Capabilities](https://youtu.be/p9T4gWds54o?t=1898)

---

## Disable Security Controls defined as Label on a Namespace
### Commands

```kubectl
kubectl label --overwrite ns ${NS} pod-security.kubernetes.io/enforce=privileged
```

### References:
[I'll Let Myself In: Kubernetes Privilege Escalation Tactics - Andrew Martin & Ian Smart](https://youtu.be/f10WQlr0h_M?t=953) a user with wildcard permissions in the same Namespace can overwrite security controls defined as labels on a namespace (KubeCon EU '24)

---
## Redirectors

- [Piko](https://github.com/andydunstall/piko?utm_source=tldrnewsletter): open-source alternative to Ngrok, designed to serve production traffic and be simple to host (particularly on Kubernetes)
- [RedGuard](https://github.com/wikiZ/RedGuard): a C2 front flow control tool,Can avoid Blue Teams,AVs,EDRs check.


## Exfiltrate secrets via DNS
- e.g. suggested in [Command and Kubectl - K8s Security for Pentesters and Defenders](https://www.canva.com/design/DAGgrY1QwQ0/HDW7_YCi5EvJ6u_GONhFow/view?utm_source=tldrsec.com&utm_medium=referral&utm_campaign=tl-dr-sec-272-ai-agent-security-kubernetes-security-state-of-cloudsec-reports-insights-or-self-owns#43)




## Interesting Ports and URLs

[Source](https://trustedsec.com/blog/kubernetes-for-pentesters-part-1)

| Port            | Process        | Description                                                            |
|-----------------|----------------|------------------------------------------------------------------------|
| 443/TCP         | kube-apiserver | Kubernetes API port                                                    |
| 2379/TCP        | etcd           | etcd,etcdAPI                                                           |
| 6666/TCP        | etcd           | etcd                                                                   |
| 4194/TCP        | cAdvisor       | Container metrics                                                      |
| 6443/TCP        | kube-apiserver | Kubernetes API port                                                    |
| 8443/TCP        | kube-apiserver | Minikube API port                                                      |
| 8080/TCP        | kube-apiserver | Insecure API port                                                      |
| 10250/TCP       | kubelet        | HTTPS API which allows full mode access                                |
| 10255/TCP       | kubelet        | Unauthenticated read-only HTTP port: pods, running pods and node state |
| 10256/TCP       | kube-proxy     | Kube Proxy health check server                                         |
| 9099/TCP        | calico-felix   | Health check server for Calico                                         |
| 6782-4/TCP      | weave          | Metrics and endpoints                                                  |
| 30000-32767/TCP | NodePort       | Proxy to the services                                                  |
| 44134/TCP       | Tiller         | Helm service listening                                                 |




### first commands after gaining access to a pod:
- `whoami`
- check api access
- `capsh --print`



### DefCon 33 K8s CTF Write-Up
https://www.skybound.link/2025/08/defcon-33-kubernetes-ctf-writeup/
7 challenges:
- Challenge 1 - diomhaireachdan by Raesene (Rory McCune)
- Challenge 2 - looking-under-rock by Rob CurtinSeufert
- Challenge 3 - hillwalker by Raesene (Rory McCune)
- Challenge 4 - shell-in-the-ghost by antitree
- Challenge 5 - terminate-transfer by Adam Crompton (@3nc0d3r)
- Challenge 6 - wizards-communicate by Jay Beale
- Challenge 7 - loworbit-kubernoodels
