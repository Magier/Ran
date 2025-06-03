# Ran


## Basics

Difference between `accessLevel` vs `Entitelement`:
- `accessLevel`: is the view of the adversary with regards to systems
- `entitlements`: are capabilities managed via RBAC 
    - maybe other forms of authorization will be added in the future?



## Things that need to be figured out
- support of optional parameters in procedures, and how these "sub-variants" can be properly executed
- How to model when a session is "escalated" from a container to a host system
- proper functional composition of all parsers and how they properly extract facts 
    - also, how is this nicely reflected in YAML, so a planner can make use of it