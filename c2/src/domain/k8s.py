from pydantic import BaseModel, model_serializer

class Container(BaseModel):
    name: str
    image: str
    cmd: list[str] | str | None = None
    args: list[str] | None = None


    @model_serializer
    def serialize(self):
        data =  {
            "name": self.name,
            "image": self.image
        }

        if self.cmd is not None:
            data["command"] = self.cmd if isinstance(self.cmd, list) else [self.cmd]
        if self.args is not None:
            data["args"] = self.args

        return data


class PodSpec(BaseModel):
    name: str
    ns: str | None = None
    containers: list[Container] = []
    host_ipc: bool | None = None
    host_pid: bool | None = None
    host_network: bool | None = None

    @model_serializer
    def serialize(self):
        data = {
            "apiVersion": "v1",
            "kind": "Pod",
            "metadata": {
                "name": self.name,
                "namespace": self.ns
            },
            "spec": {
                "containers": [],
                "securityContext": {}
            }
        }

        if self.ns is not None:
            data["metadata"]["namespace"] = self.ns

        if self.host_ipc is not None:
            data["spec"]["hostIPC"] = self.host_ipc
        if self.host_pid is not None:
            data["spec"]["hostPID"] = self.host_pid
        if self.host_network is not None:
            data["spec"]["hostNetwork"] = self.host_network

        data["spec"]["containers"] += [c.model_dump() for c in self.containers]

        return data