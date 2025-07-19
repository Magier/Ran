# Kubecon EU 2025 Hacking Up a Storm

Great tutorial at KubeCon EU 2025 by Rory McCune, Marion Mccune and Iain Smart.
The [Repository](https://github.com/Container-Security-Training/Kubecon-EU-25?tab=readme-ov-file) contains all the required materials to setup the environment (Kind Cluster), along with slides to guide the participants.

As an extra, the exploitation of CVE-2020-8554 was also part of the tutorial, but will not be used in this scenario.


## Attack Flow

Context: 
- You are a developer in the `dev` namespace, but for maintainance 
- We've just deployed a "Log reader" application in our `dev` namespace
- Designed for the critical task of - reading logs from a system pod in the `kube-system` namespace
- Sadly, the `read-log-file` deployment is not working
- We want to fix it, and _maybe_ give us more permissions along the way, to ensure we have an easier life in the future

---  
Steps: 
1) Connect to the `workstation` pod in the `dev` namespace
2) Check own permissions  
3) Check resources we can access
    - check pods
    - check deployments
    - check serviceaccounts -> find `rbac-manager` SA 
- (skip?) check logs of the `read-log-file` pod
4) Try spawn privileged container to access node
   -> fail because of PodSecurityAdmission (PSA)
5) Get a serviceaccount, which uses the `rbac-manager`
    - create pod, that mounts that serviceaccount
6) Exec that created pod to read that token
7) impersonate that serviceaccount on the `workstation` pod   (PrivEsc: Access Token Manipulation: Token Impersonation/Theft [T1134.001])
8) check these permissions -> `escalate` verb
9) Create role wildcard permissions called `nsadmin`  (PrivEsc: Access Token Manipulation: Make and Impersonate Token [T1134.003])
10) create rolebinding, assigning that role to the workstation SA (PrivEsc: Access Token Manipulation: Make and Impersonate Token [T1134.003])
11) verify elevated permissions of `dev` SA again
12) Disable PSA on namespace: Modify namespace label (Defense Evasion: Impair Defenses [T1562])
    - `kubectl label namespace dev pod-security.kubernetes.io/enforce=privileged --overwrite`
- (skip?) check if pod starts now
---   

Act 2

13) create privileged container `noderoot`  (deploy container T1610)
14) chroot onto Node    (PrivEsc Escape to Host https://attack.mitre.org/techniques/T1611/)
15) get `kubelet.conf` ?
16) discover other nodese in cluster
    - deploy nmap?
17) create new pod on the control-plane node with hostPath
18) access `admin.conf` and `super-admin.conf` 
19) impersonate these using:
    - `kubectl --kubeconfig=/etc/kubnetetes/admin.conf auth can-i --list`
    - `kubectl --kubeconfig=/etc/kubnetetes/super-admin.conf auth can-i --list`
20) impact: full cluster admin
