# Targeting

The target identifies the entity a TTP acts on. The atomic CLI currently targets pods by canonical entity ID:

```sh
ran trigger get-pods --target ns/default/pod/compromised-pod
```

In the browser UI, select an entity in the campaign graph. Ran evaluates each TTP's preconditions against the selected entity and current campaign knowledge.

The execution system can differ from the logical target when Ran has a suitable channel. `ran trigger --exec-system <entity-id>` provides an explicit override. Resource-development actions may execute locally rather than inside a target.

In effects and defaults, `sys` means the current execution system, while `${TARGET_ID}` is the selected target ID.
