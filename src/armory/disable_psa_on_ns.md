## Disable Security Controls defined as Label on a Namespace


## Commands

```kubectl
kubectl label --overwrite ns ${NS} pod-security.kubernetes.io/enforce=privileged
```

## References:
[I'll Let Myself In: Kubernetes Privilege Escalation Tactics - Andrew Martin & Ian Smart](https://youtu.be/f10WQlr0h_M?t=953) a user with wildcard permissions in the same Namespace can overwrite security controls defined as labels on a namespace (KubeCon EU '24)