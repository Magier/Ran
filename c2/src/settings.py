from pathlib import Path
from pydantic_settings import BaseSettings


class Settings(BaseSettings):
    api_port: int = 8000
    c2_ip: str | None = "172.18.0.1"
    c2_start_cmd: str | Path | None = "sliver-server daemon"
