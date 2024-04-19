from asyncio import Queue
import aiohttp

from adapters.sliver_wrapper import setup_c2_session, get_c2_client, get_file
from domain import commands, events
from domain.implant import Implant
from domain.ttps import TTP, ExploitParams, Tactic
from services.messagebus import MessageBus

from adapters.sliver_wrapper import SliverClient, SliverSession


class C2:
    def __init__(self, host_ip: str | None, file_port: int):
        self.c2_client: SliverClient = None
        self.c2_sessions: dict[str, SliverSession] = {}
        self.host_ip = host_ip
        self.file_port: int = file_port
        self.c2_listener = None
        self.variable_map = {
            "$C2": f"http://{self.host_ip}:{self.file_port}"
        }


    async def register(self, msg_bus: MessageBus) -> None:
        self.msg_bus = msg_bus

        msg_bus.register_event_handler(events.ListenerReady, self.on_listener_ready)
        msg_bus.register_event_handler(events.SessionConnected, self.add_session)
        msg_bus.register_event_handler(events.SessionDisconnected, self.remove_session)
        msg_bus.register_command_handler(commands.DeleteEntities, self.delete_sessions)
        msg_bus.register_command_handler(commands.ResetCampaign, self.reset)
        msg_bus.register_command_handler(commands.ExecuteTTP, self.execute_ttp)

    async def setup(self, queue: Queue, c2_start_cmd: str):
        await setup_c2_session(queue, c2_start_cmd)

    async def on_listener_ready(self, event: events.ListenerReady) -> events.Event | None:
        self.c2_listener = f"{event.host}:{event.port}"

    async def add_session(self, event: events.SessionConnected) -> None:
        """Keep track of new C2 sessions to delegate any commands.
        :param event: SessionConnected event with information about the C2 session
        """
        try:
            # self.c2_sessions[event.session_id] = event.system.id
            self.c2_sessions[event.system.id] = event.session_id
        except Exception as exc:
            print("ERROR adding a session: " + str(exc))
        return None

    async def remove_session(self, event: events.SessionDisconnected) -> None:
        """When a session disconnects also remove it from the tracked connections.
        :param event: SessionDisconnected event with the hostname of the disconnected system
        """
        try:
            # TODO find system ID based on name
            sys_id = next((sys for sys, c2 in self.c2_sessions.items() if c2 == event.session_id), None)
            if sys_id is None:
                print("No system for session id '{event.session_id}' found! No session was removed!")
            else:
                c2 = self.c2_sessions.pop(sys_id, None)
        except Exception as exc:
            print("ERROR removing session on '{event.system_name}': " + str(exc))
        return None

    async def delete_sessions(self, cmd: commands.DeleteEntities) -> None:
        if self.c2_client is None:
            self.c2_client = await get_c2_client()
        for e in cmd.entities:
            if e in self.c2_sessions:
                session_id = self.c2_sessions[e]
                await self.c2_client.kill_session(session_id)

    async def reset(self, cmd: commands.ResetCampaign) -> None:
        if self.c2_client is None:
            self.c2_client = await get_c2_client()
        # kill all active sessions
        for s in self.c2_sessions.values():
            await self.c2_client.kill_session(s)

    async def execute_ttp(self, cmd: commands.ExecuteTTP) -> events.Event | None:
        """Translated any commands to the C2 implants and the corresponding actions.

        :param cmd: the domain command
        :raises FileNotFoundError: if the targeted file was found at the expected location in on the system
        :return: an event with the extracted loot
        """
        if self.c2_client is None:
            self.c2_client = await get_c2_client()

        ttp = cmd.ttp # TODO just a temporary workaround till after the logic transition to campaign

        if cmd.system_id is None:
            if ttp.tactic == Tactic.InitialAccess:
                return await self.attempt_initial_access(cmd, ttp)

        if cmd.system_id not in self.c2_sessions:
            target_name = cmd.target if isinstance(cmd.target, str) else cmd.target.name
            raise NotImplementedError(f"No suitable system found from where {target_name} can be attacked!")

        session_id = self.c2_sessions[cmd.system_id]
        implant = Implant(client=self.c2_client, session_id=session_id, system_id=cmd.system_id)

        if ttp is None:
            print(f"Unknown TTP: {cmd.ttp_id} ({cmd.technique})")
            return None
        if ttp.execute is None:
            if ttp.tactic == Tactic.InitialAccess:
                return await self.attempt_initial_access(cmd, ttp)
            else:
                raise NotImplementedError(f"TTP '{ttp.name}' has no execute function implemented!")
        try:
            # TODO perform dependency injection here?
            if len(cmd.params) > 0 or ttp.params is not None:
                target, params = await self.hydrate_parameters(cmd, ttp.params)
                event = await ttp.execute(implant, cmd.target, ttp, target_url=target, params=params)
            else:
                event = await ttp.execute(implant, cmd.target, ttp)
            return event
        except Exception as exc:
            raise ValueError(f"Failed to execute TTP '{ttp.name}': {exc}")

    async def attempt_initial_access(self, cmd: commands.ExecuteTTP, ttp: TTP) -> events.Event | None:
        """Send a request to the target specified in the params which will hopefully trigger the next stage.

        :param cmd: the command with the necessary arguments for the exploit
        :return: None, as a successful exploitation should trigger a SessionConnected event
        """
        # dropper_url = f"http://{self.host_ip}:{self.file_port}"
        # if len(cmd.params) > 0:
        #     target = cmd.params["target"]
        #     exploit_params = cmd.params["params"]
        # else:
        #     # TODO: properly selected the target port and not just use the 1st
        #     target_port = list(cmd.target.ports.values())[0]
        #     target = f"http://{cmd.target.ip}:{target_port}{ttp.params.endpoint}"
        #     exploit_params = ttp.params.params
        # # replace the variable $C2 with the URL of the dropper
        # ps = {k: v.replace('$C2', dropper_url) for k, v in exploit_params.items()}

        target, params = await self.hydrate_parameters(cmd, ttp.params)

        async with aiohttp.ClientSession() as session:
            # TODO: consider the correct `method` from the ExploitParams
            print(f"attempt IA to '{target}'")
            async with session.get(target, params=params) as response:
                html = await response.text()

        return None

    async def hydrate_parameters(
        self, cmd: commands.Command, ttp_params: ExploitParams | None = None
    ) -> tuple[str, dict]:
        if len(cmd.params) > 0:
            target = cmd.params.get("target", None)
            exploit_params = cmd.params.get("params", cmd.params)
        else:
            target_host = cmd.target.ip
            # ports = list(cmd.target.ports.values())

            # TODO: properly selected the target port and not just use the 1st
            target_port = next(iter(cmd.target.ports.values()), None)
            if target_port is not None:
                target_host = f"{target_host}:{target_port}"

            target = f"http://{target_host}{ttp_params.endpoint}"
            exploit_params = ttp_params.params

        if target is not None and "$TARGET" in target:
            target = target.replace("$TARGET", cmd.target.ip)
        # replace the variable $C2 with the URL of the dropper
        for k, v in exploit_params.items():
            if isinstance(v, str):
                exploit_params[k] = v.replace('$C2', self.variable_map["$C2"])
            elif isinstance(v, list):
                exploit_params[k] = [entry.replace('$C2', self.variable_map["$C2"]) for entry in v]

        return target, exploit_params
