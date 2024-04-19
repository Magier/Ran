
from domain import k8s
import pytest


class TestPodSpecSerialization:
    def test_metadata(self):
        name = "test"
        ns = "my-ns"
        spec = k8s.PodSpec(name=name, ns=ns)
        dump = spec.model_dump()
        assert dump["metadata"]["name"] == name
        assert dump["metadata"]["namespace"] == ns

    def test_containers(self):
        name = "my-container"
        image = "img:latest"
        container = k8s.Container(name=name, image=image)
        spec = k8s.PodSpec(name="test", ns="default", containers=[container])
        dump = spec.model_dump()
        
        cs = dump["spec"]["containers"]
        assert len(cs) == 1
        assert isinstance(cs[0], dict) 

    @pytest.mark.parametrize("val", [True, False, None])
    def test_host_flags(self, val):
        spec = k8s.PodSpec(name="name", ns="ns", host_ipc=val, host_pid=val, host_network=val)
        dump = spec.model_dump()

        for field in ["hostNetwork", "hostIPC", "hostPID"]:
            if val is None:
                assert field not in dump["spec"]
            else:
                dump["spec"][field] == val

    def test_volume_mounts(self):
        assert False
    
    def test_security_context(self):
        assert False


class TestContainerSerialization:
    def test_mandatory_fields(self):
        name = "my-name"
        image = "my-image:latest"
        c = k8s.Container(name=name, image=image)
        dump = c.model_dump()
        assert dump["name"] == name
        assert dump["image"] == image

    def test_commands(self):
        cmd = ["/bin/bash"]
        c = k8s.Container(name="name", image="img", cmd=cmd)
        dump = c.model_dump()
        assert dump["command"] == cmd

    def test_command_may_be_str(self):
        cmd = "/bin/bash"
        c = k8s.Container(name="name", image="img", cmd=cmd)
        dump = c.model_dump()
        assert dump["command"] == [cmd]


    def test_args(self):
        args = ["-c", "echo"]
        c = k8s.Container(name="name", image="img", args=args)
        dump = c.model_dump()
        assert dump["args"] == args
