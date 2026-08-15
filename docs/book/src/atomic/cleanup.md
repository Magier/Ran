# Cleanup

TTPs that change the environment can declare a `cleanup:` procedure:

```yaml
procedures:
  - key: kubectl
    command: kubectl create role test-role --verb=get --resource=pods -n=${NS}

cleanup:
  command: kubectl delete role test-role -n=${NS}
```

Cleanup uses the original target, arguments, procedure context, and Kubernetes identity.

A campaign reset runs available cleanup procedures before clearing campaign state. A launch-time plan prompts for cleanup after completion:

```sh
ran emulate --plan ./plan.yaml
```

Use `--cleanup` to run that cleanup automatically:

```sh
ran emulate --plan ./plan.yaml --cleanup
```

There is no standalone `ran cleanup` command. Read-only techniques need no cleanup, and Ran cannot undo external logging or changes for which a TTP has no cleanup procedure.
