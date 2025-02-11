
## Initial Access
- Wiz [Making Sense of Kubernetes Initial Access Vectors Part 1 – Control Plane](https://www.wiz.io/blog/making-sense-of-kubernetes-initial-access-vectors-part-1-control-plane)
- Wiz [Making Sense of Kubernetes Initial Access Vectors Part 2 - Data Plane](https://www.wiz.io/blog/kubernetes-data-plane)

## Privilege Escalation
- SentinelOne [Climbing The Ladder | Kubernetes Privilege Escalation (Part 1)](https://www.sentinelone.com/blog/climbing-the-ladder-kubernetes-privilege-escalation-part-1)  
- SentinelOne [Climbing The Ladder | Kubernetes Privilege Escalation (Part 2)](https://www.sentinelone.com/blog/climbing-the-ladder-kubernetes-privilege-escalation-part-2/)


## Lateral Movement
- Wiz [NamespaceHound](https://github.com/wiz-sec-public/namespacehound)
- given: hostPath mount: put a static pod manifest on the node, which spawns a privileged pod
	- try with invalid namespace name in manifest -> visibile in k8s api?

## Discovery

- [K8spider](https://github.com/Esonhugh/k8spider): supports to scan all services installed in Kubernetes cluster and all exposed ports in service

## Interesting tools to try to support


## SSH
- [Dropbear](https://github.com/mkj/dropbear)   #lolbin

## Tunnels
- [bore](https://github.com/ekzhang/bore)  
	- alternative to ngrok