from enum import IntEnum, StrEnum, auto
from typing import Callable
from uuid import uuid4
from domain.entities import AccessLevel
from pydantic import BaseModel, Field, SerializeAsAny
from strenum import SnakeCaseStrEnum

# https://microsoft.github.io/Threat-Matrix-for-Kubernetes/


class ActionParam(BaseModel):
    pass


class ExploitParams(ActionParam):
    endpoint: str
    params: dict = {}
    method: str = "GET"
    encode: bool = True


class DeployPodParams(ActionParam):
    name: str
    image: str
    cmd: str | None = None,
    args: list[str] = []
    host_ipc: bool = False
    host_pid: bool = False
    host_network: bool = False
    volume_mounts: list[str] = []
    volumes: list[str] = []


class Tactic(SnakeCaseStrEnum):
    InitialAccess = auto()
    Discovery = auto()
    CredentialAccess = auto()
    PrivilegeEscalation = auto()
    LateralMovement = auto()
    Execution = auto()


class TTP(BaseModel):
    id: str = Field(default_factory=lambda: str(uuid4()))
    tactic: Tactic | None = None
    technique: str | None = None
    name: str
    ms_id: str | None = None
    action: str | None = None
    cmd_args: dict[str, str] = {}
    execute: Callable | None = Field(None, exclude=True)
    requires: dict[str, str | AccessLevel] = []
    params: SerializeAsAny[ActionParam] | None = None


class Technique(StrEnum):
    ExploitPublicFacingApp = "T1190"
    ExploitationForPrivilegeEscalation = "T1068"
    GatherVictimHostInformation = "T1592"
    ContainerAndResourceDiscovery = "T1613"
    FileAndDirectoryDiscovery = "T1083"
    SystemNetworkConfigurationDiscovery = "T1016"
    ContainerServiceAccount = "MS-TA9016"
    StealApplicationAccessToken = "T1528"
    PermissionGroupsDiscovery = "T1069"
    PermissionGroupsDiscovery_CloudGroups = "T1069.003"
    DeployContainer = "T1610"
