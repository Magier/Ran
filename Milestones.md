
### MVP
- [x] Support K8s-related TTPs of [Leonidas](https://github.com/WithSecureLabs/leonidas) (atomic)
- [ ] Support K8s-related TTPs of [Stratus Red Team](https://stratus-red-team.cloud/attack-techniques/kubernetes/) (atomic)
- [x] Track the executed TTPs (audit trail)
    - [x] show Flow in UI
    - [x] Export trail of executed TTP as [Attack Flow](https://center-for-threat-informed-defense.github.io/attack-flow/)

### 2nd Iteration
- [ ] support cleanup logic for every TTP
- [x] Campaign reset functionality
- [ ] option to provide [Attack Flow](https://center-for-threat-informed-defense.github.io/attack-flow/) as a plan
- [ ] Support sliver as a C2 framework 
    - add respective Procedures where necessary
- [ ] Derive produced Observable from the TTP execution and link it in the Attack Flow
- [ ] (Optional) Generate Sigma from STIX Observables?
- [ ] Support hierarchical TTPs: they use other TTPs as building blocks (akin to HTN)
    - e.g., `Install implant` could be -> `generate implant` + `download binary` + `execute binary`
- [ ] provide API to interact with Ran
- [ ] improve audit tracing
    - [ ] merge multiple (failed) attempts into 1 node? 
    - [ ] Show key value in the title, to quickly differentiate same TTPs, but different objectives

### 3rd Itration
- [ ] Map TTPs to [D3FEND](https://d3fend.mitre.org/)
- [ ] Simple Planning for a explicit goal
- [ ] Explore generation of attack trees
- [ ] [MCP](https://www.anthropic.com/news/model-context-protocol) support for "Vibe kiddies" 🤖

### 4th Iteration: First basic autonomous emulation
- [ ] Behavior Tree execution
- [ ] Construct behavior tree from observed actions (trace -> Process Tree -> Behavior Tree) 
