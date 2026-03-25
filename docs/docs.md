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




### Planner/Reasoner

There are various approaches to planning and reasoning about actions. As this is an educational project, various approaches will explored:

1) No planner: the human operator decides on single tasks (classic atomic red teaming tool)
2) Imperative Plan: execute pre-defined plan/runbook (e.g. Mitre's [Attack Flow](https://ctid.mitre.org/projects/attack-flow))
3) "Classic" AI: commonly used in games and robotics
    - Behavior Trees
    - Classical planning
    - Hierarchical Task Network (HTN)
    - Goal-Oriented Action Planning (GOAP)
5) Modern AI: Reinforcement learning, Active Inference, Hybrid systems

_Note: the evolutions are heavily inspired by book [📖 Artificial Intelligence: A Modern Approach](https://aima.cs.berkeley.edu/)_


The case for planning and acting (especially in unknown environments) is well motivated in Mitre's [📄 Automated Adversary Emulation: A Case for Planning and Acting with Unknowns](https://www.mitre.org/sites/default/files/2021-11/prs-18-0944-1-automated-adversary-emulation-planning-acting.pdf) paper

