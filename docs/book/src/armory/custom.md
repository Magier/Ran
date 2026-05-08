# Custom Armory Directory

Ran loads TTPs from its built-in armory by default. You can supplement or replace
this with your own YAML files by pointing Ran at a custom directory.

## Using `--armory`

```sh
ran emulate --armory /path/to/my-ttps
ran invoke my-custom-ttp --armory /path/to/my-ttps
```

Ran scans the directory recursively for `*.yaml` files and merges them with (or
replaces, depending on IDs) the built-in armory.

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

## ID collisions

If a custom TTP has the same `id` as a built-in TTP, the custom one wins. Use
this to override individual techniques without forking the whole armory.

## Writing your own TTPs

The full guide for writing valid YAML is in [Extending the Armory](../extending/when.md).
