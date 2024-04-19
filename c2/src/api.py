from enum import auto
from functools import lru_cache
from pathlib import Path
from typing import Annotated, Any
from domain.ttps import TTP
from pydantic import TypeAdapter
from domain import events
from fastapi import APIRouter, Depends, FastAPI, WebSocket, WebSocketDisconnect
from fastapi.staticfiles import StaticFiles
from domain.entities import Topology
from domain.events import EventType
from services.messagebus import MessageBus, get_message_bus
from strenum import PascalCaseStrEnum
from strenum.mixins import Comparable
import uvicorn
from domain.armory import Armory, get_armory
from domain import commands
from services.campaign import Campaign, get_campaign


from adapters import sliver_wrapper


router = APIRouter()


IMPLANT_DIR = Path("../static").resolve()


class UiEventType(Comparable, PascalCaseStrEnum):
    ExecuteTTP = auto()
    DeleteEntity = auto()
    ResetCampaign = auto()

    def _cmp_values(self, other):
        # enable case-insensitive comparisons
        return self.value.replace("_", "").lower(), str(other).replace("_", "").lower()


class ConnectionManager:
    def __init__(self):
        self.active_connections: list[WebSocket] = []

    async def connect(self, websocket: WebSocket):
        await websocket.accept()
        self.active_connections.append(websocket)

    def disconnect(self, websocket: WebSocket):
        self.active_connections.remove(websocket)

    async def ping(self, websocket: WebSocket):
        await websocket.send_text("ping")

    async def send_personal_message(self, message: str, websocket: WebSocket):
        await websocket.send_text(message)

    async def broadcast(self, message: str):
        print(f"[UI] 📢 {message}")
        for connection in self.active_connections:
            await connection.send_json(message)


class State:
    events: list = []


@lru_cache
def get_connection_manager():
    manager = ConnectionManager()
    return manager


@lru_cache
def get_state():
    return State()


@router.get("/sessions")
async def list_sessions():
    return await sliver_wrapper.get_sessions()


@router.websocket("/ws")
async def websocket_endpoint(
    websocket: WebSocket,
    manager: Annotated[ConnectionManager, Depends(get_connection_manager)],
    campaign: Annotated[Campaign, Depends(get_campaign)],
    msg_bus: Annotated[MessageBus, Depends(get_message_bus)],
    armory: Annotated[Armory, Depends(get_armory)],
):
    await manager.connect(websocket)
    ttps_type = TypeAdapter(list[TTP])
    # ttps_type = TypeAdapter(dict[str, list[TTP]])
    # ttps = armory.get_grouped_ttps()
    data = ttps_type.dump_python(list(armory.ttps.values()), exclude_none=True)  # convert domain models to plain python objects for subsequent serialization
    await websocket.send_json({"type": EventType.Armory, "data": data})

    # send already established topology to any new client
    topology = await campaign.get_topology()
    data = await serialize_topology(topology)
    await websocket.send_json({"type": EventType.Topology, "data": data})

    try:
        while True:
            data = await websocket.receive_json()
            print(f"💻 {data}")
            msg = await parse_message(data)

            if msg is not None:
                await msg_bus.queue.put(msg)
    except WebSocketDisconnect:
        manager.disconnect(websocket)
        await manager.broadcast(f"UI disconnected")


async def parse_message(msg: dict[str, Any]):
    event_type = msg.get("event_type", None)

    target_id = msg.get("target", None)
    if event_type == UiEventType.ExecuteTTP:
        params = msg.get("cmd_args", {}) 
        technique = msg["technique"]
        campaign = get_campaign()

        owning_system =  await campaign.get_owning_system(target_id)
        systemd_id = owning_system.id if owning_system is not None else target_id or None
        target =  campaign.entities.get(target_id, None)
        if target is None and "target" in params: # use generic target for the command (may be outside of cluster)
            target = params["target"]

        # TODO: resolve target to the owning system; as the target can be any Node in the UI
        return commands.PrepareTTP(name=technique, ttp_id=msg["ttp_id"], system_id=systemd_id, target=target, params=params)
    elif event_type == UiEventType.DeleteEntity:
        return commands.DeleteEntities(entities=[target_id])
    elif event_type == UiEventType.ResetCampaign:
        return commands.ResetCampaign()
    return commands.UnknownCommand(system_id=target_id, name="?")


async def serialize_topology(topo: Topology) -> dict[str, list]:
    entities = [e.model_dump(exclude_none=True) for e in topo.entities]
    # TODO: just a dirty workaround, properly implement resolution of hierarchies
    entity_map = {f"{e.get('kind', '?')}/{e['name']}": e["id"] for e in entities}

    # for the UI reference the namespace by it's UUID
    for e in entities:
        ns = e.get("ns", None)
        match ns:
            case dict():
                e["ns"] = ns["id"]
            case str():
                if ns != "?":
                    e["ns"] = entity_map[f"Namespace/{ns}"]
    relations = [r.model_dump(exclude_none=True) for r in topo.relations]
    return {"entities": entities, "relations": relations}


async def handle_ui_event(event: events.Event):
    conn_mngr = get_connection_manager()
    data = event.data
    if event.type == EventType.Topology:
        data = await serialize_topology(event.data)
    envelope = {"type": str(event.type), "data": data}
    await conn_mngr.broadcast(envelope)


def create_app(msg_bus: MessageBus) -> FastAPI:
    msg_bus.register_event_handler(events.UiEvent, handle_ui_event)

    app = FastAPI()
    app.include_router(router)  # , dependencies=[Depends(_get_msg_bus)])

    # serve the implants
    app.mount("/static", StaticFiles(directory=IMPLANT_DIR), name="implants_dir")
    # app.add_api_websocket_route("/ws", websocket_endpoint, dependencies=[Depends(_get_msg_bus)])
    return app


async def start_api(msg_bus: MessageBus, port: int = 8000, reload: bool = True):
    """Launched with `poetry run start` at root level"""
    app = create_app(msg_bus=msg_bus)
    config = uvicorn.Config(app, host="0.0.0.0", port=port, reload=reload)
    server = uvicorn.Server(config)

    app.msg_bus = msg_bus
    await server.serve()
