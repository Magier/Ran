# Cleanup

Some techniques leave side-effects in the cluster: created roles, role bindings,
deployed pods, injected configuration. Cleanup procedures reverse those changes.

## How cleanup is declared

A TTP can declare a single `cleanup:` procedure alongside its regular `procedures:`:

```yaml
procedures:
  - key: kubectl
    command: kubectl create role nsadmin --verb=* --resource=* --token=${TOKEN} -n=${NS}

cleanup:
  command: kubectl delete role nsadmin --token=${TOKEN} -n=${NS}
```

The cleanup procedure follows the same format as a regular procedure (shell command,
`k8s_request`, `http_request`, or `steps`).

## Running cleanup in the web UI

After a technique executes successfully, a **Clean Up** button appears in the
execution panel. Clicking it runs the cleanup procedure against the same target
and with the same parameters that were used during execution.

## Running cleanup on the CLI

```sh
ran cleanup <ttp-id> --target <namespace>/<pod>
```

This invokes the cleanup procedure for the named TTP.

## When cleanup matters

Not all techniques need cleanup. Read-only discovery techniques (listing pods,
enumerating RBAC) leave no trace in the cluster and have no cleanup procedure.
Techniques that create or modify cluster resources — creating roles, binding service
accounts, spawning pods — should declare cleanup so you can restore the cluster to
its original state after testing.

## What cleanup does not cover

Cleanup reverses the *direct* cluster-side effect of the TTP. It does not:

- Remove entries from Ran's internal knowledge graph (the campaign state)
- Delete logs or events already captured by your monitoring stack
- Undo changes to external systems (e.g. cloud provider APIs)

If you need a full environment reset, rebuild your test cluster from scratch.
