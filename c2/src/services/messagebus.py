import asyncio
from asyncio import Queue
from collections import defaultdict
from functools import lru_cache
from typing import Callable
from domain import events
from domain.commands import Command
from domain.events import Event, EventType


class MessageBus:
    def __init__(self):
        self.queue: Queue = asyncio.Queue()
        self.event_handlers = defaultdict(list)
        self.command_handlers = defaultdict(list)

    def register_event_handler(self, event_type: Event, handler: Callable) -> None:
        self.event_handlers[event_type].append(handler)

    def register_command_handler(self, cmd_type: Command, handler: Callable) -> None:
        self.command_handlers[cmd_type].append(handler)

    async def setup(self):
        # TODO properly init c2 and then start event handlers
        # wait for setup to be done
        # ready = await self.queue.get()
        # print(f"Queue: {ready}")

        asyncio.create_task(self.handle_messages(self.queue))

    async def handle_messages(self, queue: asyncio.Queue[Event | Command]) -> None:
        while True:
            msg = await queue.get()
            event_name = type(msg).__name__
            print(f"💻 RX {event_name}")

            is_cmd = isinstance(msg, Command)
            handler_index = self.command_handlers if is_cmd else self.event_handlers

            handlers = handler_index.get(type(msg), [])
            if len(handlers) == 0:
                print(f"Unhandled msg {type(msg)}: {msg}")

            for h in handlers:
                try:
                    res = await h(msg)
                    if res is not None:
                        await queue.put(res)
                except Exception as exc:
                    msg = f"Handler '{h.__name__}' could not handle event '{event_name}': {exc}"
                    # TODO maybe handle this event also for the error handling the CLI
                    print(msg)
                    # TODO this can lead to infinite loop, if any error handler also produces an error# Todo: this can lead to infinite loop, if any error handler also produces an error# Todo: this can lead to infinite loop, if any error handler also produces an error
                    ev = events.UiEvent(type=EventType.Error, data=msg)
                    await queue.put(ev)


@lru_cache
def get_message_bus() -> MessageBus:
    return MessageBus()
