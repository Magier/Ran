from enum import IntEnum, auto
from uuid import uuid4
from pydantic import BaseModel, Field
from strenum import PascalCaseStrEnum


class AccessLevel(IntEnum):
    Nil = 0
    UserRead = 1
    UserWrite = 2
    UserExecute = 3
    RooWriteRead = 1
    RooWriteWrite = 2
    RooWriteExecute = 3


class Entity(BaseModel):
    id: str = Field(default_factory=lambda: str(uuid4()))
    name: str
    aliases: list[str] = []


class C2(Entity):
    pass


class Asset(BaseModel):
    pass


class System(Entity):
    # TODO maybe add UID, GID, arch,
    name: str
    ip: str | None = None
    os: str = "linux"
    username: str | None = None
    version: str | None = None
    transport: str | None = None
    ports: dict[int | str, int] = {}  # key is either port name or port number
    access_level: AccessLevel = AccessLevel.Nil


# JWT RFC: https://datatracker.ietf.org/doc/html/rfc7519#section-4
class JWTToken(Asset):
    subject: str | None = None
    audience: list[str] = []
    issuer: str | None = None
    expires_at: int | None = None
    issued_at: int | None = None
    not_valid_before: int | None = None
    raw: str | None = None


class ServiceAccountToken(JWTToken):
    # TODO verify if issuer is indicater of K8s api server?
    namespace: str | None = None
    pod_name: str | None = None
    pod_uid: str | None = None
    serviceaccount_name: str | None = None
    serviceaccount_uid: str | None = None
    warn_after: int | None = None


# TODO differentiate between k8s resources and a IAM entity?
class ServiceAccount(Entity):
    kind: str = "ServiceAccount"
    ns: str
    name: str
    token: str | ServiceAccountToken | None = Field(None, exclude=True)
    expires_at: int | None = None
    can: list[str] = []


class Namespace(Entity):
    kind: str = "Namespace"


class Pod(System):
    kind: str = "Pod"
    name: str
    ns: str | Namespace | None = None
    service_account: ServiceAccount | None = None 


class Service(Entity):
    kind: str = "Service"
    name: str
    ip: str | None = None
    ports: dict[int | str, int] = {}  # key is either port name or port number
    ns: str | Namespace | None = None


class ApiServer(System):
    kind: str = "KubeApiServer"
    name: str = "API Server"
    ns: str = "kube-system"


class Relation(BaseModel):
    name: str
    source: str
    destination: str
    data: str | None = None


class SystemState(PascalCaseStrEnum):
    Unknown = auto()
    Known = auto()
    Reachable = auto()
    Compromised = auto()


class Topology(BaseModel):
    entities: list[Entity] = []
    relations: list[Relation] = []
