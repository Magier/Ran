from dataclasses import dataclass
from typing import Any


@dataclass
class Implant:
    client: Any
    session_id: str
    system_id: str
