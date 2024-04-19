from domain.entities import Entity
from domain.ttps import TTP, Technique
from pydantic import BaseModel, ConfigDict


# TODO this approach must work also with an extensible armory

# TODO: add action to check `kubectl auth can-i --list` with the given token


class Command(BaseModel):
    system_id: str | None = None
    name: str


class PrepareTTP(Command):
    ttp_id: str
    target: Entity | str | None  # depending on the TTP the target may be inferred
    technique: Technique | None = None
    params: dict | None = {}

class ExecuteTTP(Command):
    ttp: TTP
    target: Entity | str | None  # depending on the TTP the target may be inferred
    technique: Technique | None = None
    params: dict | None = {}


class DeleteEntities(Command):
    name: str = "delete_entities"
    entities: list[str]


class ResetCampaign(Command):
    name: str = "reset"


class AnalyzeEnvironmenVariables(Command):
    pass


class UnknownCommand(Command):
    model_config = ConfigDict(extra='allow')
