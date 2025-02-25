## TTP



### Ktrust TTPs
- basically MS Threat Matrix for Kubernetes

#### Initial Access
- SSH Server running inside container 
    - (not in MS Threat Matrix)
- Exposed Sensitive Interfaces
- Application Vulnerability
- Kubeconfig File
- Compromised image in registry
- Using Cloud Credentials


#### Persistence
- Static Pods
- Malicious Admission Controller
- Kubernetes Cronjob
- Container Serivce Account
- Writable hostPath mount
- Backdoor Container

#### Credential Access
- Access Managed Identity Credentials
- Application Credentials in Configuration Files
- Malicious Admission Controller
- Container Service Account
- Access Node Information
    - (mount service principal)
- List K8s Secrets


#### Lateral Movement
- ARP Poisining and IP Spoofing
- CoreDNS Poisoning
- Cluster Internal Networking
- Application Credentials in Configuration Files
- Container Service Account
- Access Cloud Resources
- Writable hostPath mount


#### Execution
- Sidecar Injection
- Application Exploit (RCE)
- New Container
- Exec Inside Container

- (bash /cmd inside container)
- (ssh servere running inside container)



#### Privilege Escalation
- hostPath Mount
- Cluster-admin binding
- privileged Container
- Access Cloud Resources


#### Defense Evasion
- Connect From Proxy Server
- Pod Name Similarity
- Delete Events
- Clear Container Logs


#### Discovery
- Instance Metadata API
- Network Mapping
- Access Kubelet API
- Access Kubernetes API Server
- Exposed Sensitive Interfaces


#### Collection
- Collecting Data From Pod
- Images from Private Registry


#### Impact
- DoS
- Resources Hijacking
- Data Destruction


## Microsoft Threat Matrix for Kubernetes
![alt text](image.png)