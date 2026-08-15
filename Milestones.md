# Milestones

### MVP

- [x] Support Kubernetes-related TTPs from [Leonidas](https://github.com/WithSecureLabs/leonidas) for atomic testing
- [ ] Support Kubernetes-related TTPs from [Stratus Red Team](https://stratus-red-team.cloud/attack-techniques/kubernetes/) for atomic testing
- [x] Track executed TTPs as a campaign trail
  - [x] Show the campaign flow in the UI
  - [x] Export the trail as Ran JSON

### 2nd Iteration

- [ ] Support cleanup logic for every TTP
- [x] Campaign reset functionality
- [x] Execute pre-defined YAML campaign plans
- [x] Provide REST, SSE, and MCP interfaces
- [ ] Import and export MITRE Attack Flow/STIX
- [ ] Support Sliver as a C2 framework through RPC
- [ ] Provide WebSocket RPC with a purpose-built protocol
- [ ] Derive observables from TTP execution and link them in the campaign trail
- [ ] Optionally generate Sigma rules from observables
- [ ] Support hierarchical TTPs that use other TTPs as building blocks
  - For example, install an implant by generating it, transferring it, and executing it
- [ ] Improve audit tracing
  - [ ] Merge repeated failed attempts where that improves readability
  - [ ] Surface important argument values in step titles

### 3rd Iteration

- [ ] Map TTPs to [D3FEND](https://d3fend.mitre.org/)
- [ ] Add simple planning for an explicit goal
- [ ] Explore generation of attack trees

### 4th Iteration: Basic autonomous emulation

- [ ] Behavior Tree execution
- [ ] Construct Behavior Trees from observed campaigns
