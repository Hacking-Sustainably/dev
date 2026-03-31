import os
import platform
from pathlib import Path

def get_database_path():
    system = platform.system()
    
    if system == "Linux":
        db_dir = Path("/var/lib/greenb")
    elif system == "Darwin":
        db_dir = Path.home() / "Library/Application Support/greenb"
    elif system == "Windows":
        db_dir = Path("C:/ProgramData/GreenB")
    else:
        # fallback for other systems
        db_dir = Path.home() / ".greenb"
    
    # create directory if it doesnt exist
    db_dir.mkdir(parents=True, exist_ok=True)
    
    return db_dir / "energy_monitor.db"

class Config:
    SECRET_KEY = os.environ.get("SECRET_KEY", "dev-secret-key")
    default_database_dir = f"sqlite:///{get_database_path()}"
    SQLALCHEMY_DATABASE_URI = os.environ.get("GREENB_DATABASE_URL", default=default_database_dir)
    SQLALCHEMY_TRACK_MODIFICATIONS = False
    MAX_CONTENT_LENGTH = 200 * 1024 * 1024


class DevelopmentConfig(Config):
    DEBUG = True


class ProductionConfig(Config):
    DEBUG = False


config_map = {
    "development": DevelopmentConfig,
    "production": ProductionConfig,
}
