import asyncio
from adapters.sliver_wrapper import setup_c2_session

from api import start_api
from services.c2 import C2
from services.campaign import get_campaign
from services.messagebus import MessageBus, get_message_bus
from settings import Settings


# Planning vs acting
# planning: finding what actions to perform
# acting: how to refine chosen actions into commands


async def main():
    settings = Settings()

    msg_bus = get_message_bus()
    campaign = get_campaign()
    c2 = C2(host_ip=settings.c2_ip, file_port=settings.api_port)

    #TODO: alert if sliver is not installed

    async with asyncio.TaskGroup() as tg:
        await c2.register(msg_bus=msg_bus)
        await campaign.register(msg_bus=msg_bus)
        # tg.create_task(c2.setup(msg_bus.queue, settings.c2_start_cmd))
        tg.create_task(msg_bus.setup())
        tg.create_task(start_api(msg_bus, port=settings.api_port))


if __name__ == "__main__":
    asyncio.run(main())
