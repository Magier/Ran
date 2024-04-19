from dataclasses import dataclass
from enum import StrEnum, auto
from typing import Any

from domain import System, Topology
from domain.entities import Asset, Entity, Relation, ServiceAccount, SystemState
from pydantic import BaseModel


class Event(BaseModel):
    pass


class FieldEvent(BaseModel):
    pass


class ListenerReady(FieldEvent):
    type: str
    host: str
    port: int


class ServiceAccountTokenExtracted(FieldEvent):
    source_system_id: str
    token: str


class EnvironmentVariablesExtracted(FieldEvent):
    source_system_id: str
    variables: dict[str, str]


class TokenCapabilitiesQueried(FieldEvent):
    source_system_id: str
    service_account: ServiceAccount
    resource_rules: list[dict[str, str | Any]] = []
    non_resource_rules: list[dict[str, str | Any]] = []


class ResourceCreated(FieldEvent):
    resource: Entity | dict


class SystemBinariesListed(FieldEvent):
    source_system_id: str
    binaries: dict[str, str]


class SessionEvent(FieldEvent):
    session_id: str


class SessionConnected(SessionEvent):
    system: System


class SessionDisconnected(SessionEvent):
    system_name: str


class SystemStateChanged(Event):
    system_id: str
    state: SystemState = SystemState.Known


class NewFacts(Event):
    data: list[Entity | Relation | Asset] = []
    entities: list[Entity] = []
    relations: list[Relation] = []
    assets: list[Asset] = []


class EventType(StrEnum):
    Armory = auto()
    Topology = auto()
    AddSubGraph = auto()
    RemoveSubGraph = auto()
    Error = auto()


# events passed to any user interface
class UiEvent(Event):
    type: str | None = "event"
    data: str | dict | Topology


# events within the campaign and C2 orchestraton
class DomainEvent(Event):
    pass
