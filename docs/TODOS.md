# TODOs

- constraints like "runs-on": a pod can only run on 1 node, if there is already another node, these must be the same

- files are always interpreted as binaries?

- make session an attribute of a system
- model the container escape as a switch from one system to another
- upload binary:
    support local file picker
- fix `grep` hack in c2.go when executing TTP
- pivot mechanism
    - have primary targeted system 

- make sure the pod spawned by the TTP to get an SA is considered `IsRunning=false`
- support variables in effects

- mounts: how to identify if it's a directory? 
    - don't provide potion to read file, if it's a directory

- get volumemount with hostpath -> kubelet gets more nodes, with unknown node
    - after getting proper node info, update all the other relations and entities as well (instead of pointing to unknown node)

- [UI] decouple the `Tree` UI component from the `Mount` types
- for chained execution channel, properly wrap the commands and the returned errors.

- [Sliver] sliver-c2-channel should go from listener to the target, not from sliver itself

- create callbacks for entitlement-related relations
    - identified entitlements: analyze against entities in KB
    - register callback for new entities of same type
    - when new entities are added, check if they match the entitlements (see `syncCapabilities`)

- properly parse effects from
    - SideCar Injection

- [Tracing] establish link between results from an executed TTP and the input for a follow-up TTP
    - maybe make this explicit as "Condition" nodes in between the actions in the UI?


- [UI] track which entities are under C2 control
   - new entities added to the target environment (and may need to be cleaned up) 
   - which ones are only known, but not yet interacted with


- [UI] abstract the regular permissions associated with system:authenticated into the respective groups
    - [Docs](https://kubernetes.io/docs/reference/access-authn-authz/rbac/#default-roles-and-role-bindings)


- [UI] Use state to improve UX
    - [UI] order procedures depending of the availability of necessary binaries
    - [UI] suggest tokens based on the prerequisites of the TTP
        - e.g. if a TTP requires a token with `rbac: can get pods`, then suggest tokens that have this permission
        - support SAs, which are not yet "appraised" 


- support array of strings when parsing TTPs
    - e.g. `command`: `["a", "b"]` instead of `a b`

- explore the `kubectl attach` command for interactive sessions
- explore the `kubectl debug` to copy and modify a pod

- in `kubectl debug` profiles, show options options  and support constraints of each
    - e.g. if PSA `baseline` is applied on the NS, then at most `baseline` profile will work, not `sysadmin`, etc.

- using K8s-API to get resource kind should do a sync instead of just adding the resources
    - e.g. if a pod was deleted in the maintime, the `k get pods` TTP should not return it

- rework the targeting system:
    - execute from "closest" compromised container (if any)
    - what to do with selected target depends on TTP
        - e.g. create workload to get token for a role targets the role, but is executed on the "closest" compromised container

- make TTP based on others with pre-filled args
- properly implement loading TTPs from `tools`



- trying to enumerate SA names: parse the error message  
```
Error from server (Forbidden): pods "developer-70634" is forbidden: error looking up service account default/rbac-manager: serviceaccount "rbac-manager" not found
command terminated with exit code 1: 'Error from server (Forbidden): pods "developer-70634" is forbidden: error looking up service account default/rbac-manager: serviceaccount "rbac-manager" not found
```