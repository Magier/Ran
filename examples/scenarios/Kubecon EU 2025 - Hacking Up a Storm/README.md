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
7) impersonate that serviceaccount on the `workstation` pod
8) check these permissions  -> `escalate` verb
9) Create role wildcard permissions called `nsadmin`
10) create rolebinding, assigning that role to the workstation SA 
11) check permissions of original SA again
12) Disable PSA on namespace: Modify namespace label 
13) (skip?) check if pod starts now
---   

Act 2

14) create privileged container `noderoot`
15) chroot onto Node
16) access `admin.conf` and `super-admin.conf`
17) impersonate these using:
    - `kubectl --kubeconfig=/etc/kubnetetes/admin.conf auth can-i --list`
    - `kubectl --kubeconfig=/etc/kubnetetes/super-admin.conf auth can-i --list`
18) impact: full cluster admin
