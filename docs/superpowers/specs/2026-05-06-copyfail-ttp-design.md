# CopyFail (CVE-2026-31431) TTP Design

## Overview

Add a new TTP for the CopyFail kernel vulnerability that allows a fully unprivileged container to escape to the host by corrupting shared page-cache entries used by a privileged DaemonSet.

- **File**: `armory/TTPs/Privilege Escalation/Escape to Host/copyfail.yaml`
- **CVE**: CVE-2026-31431
- **Reference PoC**: https://github.com/Percivalll/Copy-Fail-CVE-2026-31431-Kubernetes-PoC

---

## Vulnerability

An `AF_ALG` splice race in the Linux kernel's page-cache Copy-on-Write path allows an unprivileged process to corrupt in-memory cached pages of read-only files. Because container runtimes share overlay filesystem layers, a container can corrupt binaries belonging to a privileged DaemonSet (kube-proxy, node-exporter, etc.) running on the same node. When the DaemonSet next executes the corrupted binary, the attacker's payload runs with the DaemonSet's full privileges and host namespace access.

**Vulnerable kernels**: < 6.6.89, < 6.12.80

---

## TTP Metadata

```yaml
name:      Escape container via CopyFail (CVE-2026-31431)
tactic:    Privilege Escalation
techniques: ["Escape to Host", T1611]
status:    draft
effects:   [container.escape(sys)]
```

---

## Preconditions

```yaml
preconditions:
  accessLevel: "user-exec"
  # kernel: "<6.6.89 || <6.12.80"  -- enforced once framework gains kernel-version awareness
```

The pod requires no special capabilities, no root, and no host mounts — this is what makes CopyFail notable. `examples/Simple_Env/2_worker.yaml` (non-root, drops ALL caps, `allowPrivilegeEscalation: false`, seccomp `RuntimeDefault`) is the canonical victim pod.

---

## Parameters

| Name | Type | Required | Default | Description |
|------|------|----------|---------|-------------|
| `KERNEL_VERSION` | string | false | — | Kernel version of the target node. Cosmetic today; used for documentation and future precondition enforcement. Vulnerable ranges: `<6.6.89` or `<6.12.80`. |
| `PAYLOAD` | string | false | `hostname` | Command to execute in the host context once a privileged binary is corrupted and triggered. |

---

## Procedures

### Procedure 1 — Inline Python PoC

An `execute: { lang: python }` inline script (same pattern as IngressNightmare). The script:

1. Scans for target binaries present in shared image layers (e.g. `/usr/sbin/xtables-legacy-multi`, `/usr/sbin/nft`, `/usr/sbin/ipset`) that are likely also present in a privileged DaemonSet image
2. Opens the target binary read-only and pins the page-cache entry
3. Creates an `AF_ALG` AEAD socket via `ctypes` raw syscall wrappers
4. Races `splice(2)` writes through the AF_ALG socket to overwrite the cached pages with the attacker payload in-place — without filesystem write permission
5. Re-reads the file to confirm corruption landed
6. Exits; corruption persists in kernel memory until the DaemonSet executes the binary

Status is `draft` because the splice race is timing-sensitive and architecture-dependent. The script is a structurally faithful skeleton; actual reliability depends on kernel version and workload scheduling.

### Procedure 2 — Ran Implant Stub

```yaml
- key: ran-implant
  command: "ran-implant --exploit copyfail --payload ${PAYLOAD}"
```

A plain `command:` stub representing a future Ran-managed implant binary that bundles this exploit alongside others. Does not exist yet; present so the procedure appears in the UI and campaign graph.

---

## Example Manifest

No new manifest needed. `examples/Simple_Env/2_worker.yaml` demonstrates the exact attack surface: a fully hardened pod (non-root, all caps dropped, no privilege escalation, seccomp RuntimeDefault) that is nonetheless vulnerable due to the kernel bug.

---

## No Defense Block

Detection is omitted. The kernel-level AF_ALG + splice signal and DaemonSet binary integrity monitoring are not yet modelled in the framework.

---

## References

- https://github.com/Percivalll/Copy-Fail-CVE-2026-31431-Kubernetes-PoC
- https://attack.mitre.org/techniques/T1611/
