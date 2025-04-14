# Comparision to other tools

Of course, Ran is not the only adversary emulation or attack path analysis tool for Kubernetes.
There are a lot of other great projects out there. To give you 

## Adversary Emulation

Emulation types: https://ctid.mitre.org/resources/adversary-emulation-library/
- Atomic
- Planned (Micro and full)
- MDP


| Tool                                                                                           | Emulation Types | Creator     | Description                                                                                     |
|------------------------------------------------------------------------------------------------|-----------------|-------------|-------------------------------------------------------------------------------------------------|
| Atomic Red Team                                                                                | Atomic          | RedCanary   | A library of simple, atomic tests mapped to the MITRE ATT&CK framework.                         |
| Stratus Red Team                                                                               | Atomic          | DataDog     | TTPs for K8s, 60+ TTPs for Cloud providers (focus is AWS)                                       |
| [Leonidas](./leonidas_ref.md)                                                                  | Atomic          | WithSecure  |                                                                                                 |
| [TTPForge](https://github.com/facebookincubator/TTPForge)                                      | Atomic          | Facebook    |                                                                                                 |
| Caldera                                                                                        | Planned         | Mitre       |                                                                                                 |
| Peirates                                                                                       | Planned         | InGuardians |                                                                                                 |
| KubeHound                                                                                      | Planned         | WithSecure  |                                                                                                 |
| KubeHunter                                                                                     | Planned         | Aqua        |                                                                                                 |
| [RedKube](https://github.com/lightspin-tech/red-kube)                                          |                 | Lightspin   | Last updated 2021                                                                               |
| [light-k8s-attack-simulations](https://github.com/lightspin-tech/light-k8s-attack-simulations) |                 | Lightspin   | contains cases to simulate an unusual/malicious behavior in linux containers; Last updated 2022 |



### Commercial
| Tool                                                                | Emulation Types | Description |
|---------------------------------------------------------------------|-----------------|-------------|
| [Prelude](https://www.preludesecurity.com/)                         |                 |             |
| [KTrust](https://www.ktrust.io/)                                    |                 |             |
| [Mitigant](https://www.mitigant.io/en)                              |                 |             |
| [NodeZero](https://www.horizon3.ai/nodezero/kubernetes-pentesting/) |                 |             |

## Attack Path Analysis

- KubeHound
- IceKube





If you feel that we are missing a comparison to another tool, please open an issue or a pull request.

