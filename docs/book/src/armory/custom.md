# Custom Armory Directory

Ran loads TTPs from its built-in armory by default. You can supplement or replace
this with your own YAML files by pointing Ran at a custom directory.

## Using `--armory`

```sh
ran emulate --armory /path/to/my-ttps
ran trigger my-custom-ttp --armory /path/to/my-ttps \
  --target ns/default/pod/my-pod
```

Ran scans the selected directory recursively for `*.yaml` files. Release
binaries keep the embedded armory and append the selected directory. Development
builds without the bundled-armory feature load the selected directory alone.
Avoid duplicate IDs: when a release has both, the embedded TTP is resolved first.

## Organising your custom armory

Follow the same tactic-directory convention as the built-in armory:

```text
my-ttps/
├── Discovery/
│   └── enumerate_custom_crds.yaml
├── Privilege Escalation/
│   └── exploit_internal_api.yaml
└── Impact/
    └── corrupt_etcd_backup.yaml
```

If a YAML file has no `tactic:` field, the tactic is inferred from its parent
directory name.

## Writing your own TTPs

The full guide for writing valid YAML is in [Extending the Armory](../extending/when.md).
