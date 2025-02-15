# Ran

Ran is an experimental offensive tool for Kubernetes clusters. It has two main objectives:
- enable quick (realistic) emulation of adversary techniques with predefined actions
- a collection for known attack vectors in Kubernetes (i.e. the implementation of the aforementioned actions)


The name is inspired by Rán, the Norse goddess of the sea (meaning 'plundering', 'theft' or 'robbery' in old Norse), who is associated with sea storms and drowned death.
She is symbolized by a net, which she uses to ensnare and pull the unwary into the depths of the ocean.

<p align="center">
<img src="./docs/Ran.svg" width="128"/>
</p>

❗ This tool is only intended for educational/demonstration purposes! Any other usage is highly discouraged.

## Motivation

In security, one often hears the cliche of the "attacker's advantage": 
> an attacker has to be right once, but a defender has to be right all the time

This statement may hold true for the `initial access` phase, but it's often overlooked, that the script flips after the IA: defenders (theoretically) have full knowledge/visibility of the environment, while an attacker has to learn and explore the environment, first.
This fundamental advantage for the defender is often overlooked and neglected, when focusing on just [atomic testing](https://medium.com/mitre-engenuity/ahhh-this-emulation-is-just-right-introducing-micro-emulation-plans-7bf4c26451d3) of TTPs.



Ran consists of 2 major components:
- C2: a classic command & control component executing given actions and manages implants, etc.
- Planner: component deliberating what actions to execute and in what order. Maturity levels:
    1) No planner: a human operator 
    2) Imperative Plan: pre-defined plan, that will be executed (i.e. a runbook)
    3) classic planning
    4) Deterministic AI: 
        - Behavior Trees
        - (Goal-Oriented Action Planning) GOAP
    5) Reasoning (Probabilistic AI) (RL, LLM, Active Inference)


This approach is [📄 Automated Adversary Emulation: A Case for Planning and Acting with Unknowns](https://www.mitre.org/sites/default/files/2021-11/prs-18-0944-1-automated-adversary-emulation-planning-acting.pdf)


## For Defenders



## Setup

- Install Sliver
    - generate a operator configuration, so Ran can act as a client


## Similar Projects

Ran is heavily inspired by similar tools in this domain, such as:
- [Caldera](https://github.com/mitre/caldera)
- [Peirates](https://github.com/inguardians/peirates)
- [Kubesploit](https://github.com/cyberark/kubesploit)
- [kube-hunter](https://github.com/aquasecurity/kube-hunter)
- [deepce](https://github.com/stealthcopter/deepce)
- [kdigger](https://github.com/quarkslab/kdigger)
- [MKAT](https://github.com/DataDog/managed-kubernetes-auditing-toolkit/)
- [kubeletmein](https://github.com/4ARMED/kubeletmein)
- [CDK - Zero Dependency Container Penetration Toolkit](https://github.com/cdk-team/CDK/)
- [red-kube](https://github.com/lightspin-tech/red-kube)
- [kubestroyer](https://github.com/Rolix44/Kubestroyer)
- [Leonidas](https://github.com/WithSecureLabs/leonidas)
- [IceKube](https://github.com/WithSecureLabs/IceKube)


#### Tools for potential support
- https://github.com/vulsio/go-exploitdb
- [PEASS-ng](https://github.com/peass-ng/PEASS-ng)
- [go-pillage-registries](https://github.com/nccgroup/go-pillage-registries)
- [amicontained](https://github.com/genuinetools/amicontained)
- [dopwn](https://github.com/4ARMED/dopwn)
- [botb](https://github.com/brompwnie/botb)
- [MTKPI](https://github.com/r0binak/MTKPI) Multi Tool Kubernetes Pentest Image 
- []

## Armory

### Container Escape
- https://github.com/danielsagi/kube-pod-escape

- https://github.com/aws-samples/hardeneks


### Defense Evasion
- https://github.com/m0nad/Diamorphine/tree/master


## LolBINS
- [GTFOBins](https://gtfobins.github.io): list of Unix binaries that can be used to bypass local security restrictions in misconfigured systems
- [LOTTunnels](https://lottunnels.github.io): Living Off The Land Tunnels
- [LOLC2](https://lolc2.github.io/)
- [LOLRMM](https://lolrmm.io/)



## Roadmap:

- [ ] Track the executed TTPs (audit trail)
- [ ] Export trail of executed TTP as [Attack Flow](https://center-for-threat-informed-defense.github.io/attack-flow/)
- [ ] Derive produced observable from the TTP execution and link it in the Attack Flow
- [ ] support examples of BishopFox' [BadPods](https://bishopfox.com/blog/kubernetes-pod-privilege-escalation)
- [ ] Support hierarchical TTPs: they use other TTPs as building blocks (akin to HTN)
    - e.g., `Install implant` could be -> `generate implant` + `download binary` + `execute binary`
- [ ] create API server for the execution layer

## References

[Raesene KubeSecurity Lab](https://github.com/raesene/kube_security_lab/tree/main)