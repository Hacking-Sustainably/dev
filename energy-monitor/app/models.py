from datetime import datetime
from app import db


class MonitoringSession(db.Model):
    """A monitoring session represents a collection period (e.g. one upload)."""
    __tablename__ = "monitoring_sessions"

    id = db.Column(db.Integer, primary_key=True)
    name = db.Column(db.String(255), nullable=False, default="Unnamed Session")
    device_name = db.Column(db.String(255), nullable=True)
    os_name = db.Column(db.String(100), nullable=True)
    os_version = db.Column(db.String(100), nullable=True)
    started_at = db.Column(db.DateTime, nullable=False, default=datetime.utcnow)
    last_sample = db.Column(db.DateTime, nullable=True)
    ended_at = db.Column(db.DateTime, nullable=True)
    created_at = db.Column(db.DateTime, nullable=False, default=datetime.utcnow)

    samples = db.relationship(
        "EnergySample", backref="session", lazy="dynamic",
        cascade="all, delete-orphan",
    )

    def duration_seconds(self):
        if self.ended_at and self.started_at:
            return (self.ended_at - self.started_at).total_seconds()
        elif self.last_sample and self.started_at:
            return (self.last_sample - self.started_at).total_seconds()
        return None

    def to_dict(self):
        return {
            "id": self.id,
            "name": self.name,
            "device_name": self.device_name,
            "os_name": self.os_name,
            "os_version": self.os_version,
            "started_at": self.started_at.isoformat() if self.started_at else None,
            "ended_at": self.ended_at.isoformat() if self.ended_at else None,
            "duration_seconds": self.duration_seconds(),
            "sample_count": self.samples.count(),
        }


class EnergySample(db.Model):
    """Individual energy measurement for a specific application at a point in time."""
    __tablename__ = "energy_samples"

    id = db.Column(db.Integer, primary_key=True)
    session_id = db.Column(
        db.Integer, db.ForeignKey("monitoring_sessions.id"), nullable=False,
    )

    timestamp = db.Column(db.DateTime, nullable=False, index=True)
    app_name = db.Column(db.String(255), nullable=False, index=True)
    pid = db.Column(db.Integer, nullable=True)

    # Energy metrics
    power_watts = db.Column(db.Float, nullable=True)       # instantaneous power draw (W)
    energy_joules = db.Column(db.Float, nullable=True)      # energy consumed in interval (J)
    cpu_percent = db.Column(db.Float, nullable=True)        # CPU utilisation %
    memory_mb = db.Column(db.Float, nullable=True)          # memory usage in MB
    gpu_percent = db.Column(db.Float, nullable=True)        # GPU utilisation %
    disk_read_mb = db.Column(db.Float, nullable=True)       # disk read in MB
    disk_write_mb = db.Column(db.Float, nullable=True)      # disk write in MB
    network_sent_mb = db.Column(db.Float, nullable=True)    # network sent in MB
    network_recv_mb = db.Column(db.Float, nullable=True)    # network received in MB

    # Classification
    category = db.Column(db.String(100), nullable=True)     # e.g. "browser", "ide", "game"
    is_background = db.Column(db.Boolean, default=False)

    def to_dict(self):
        return {
            "id": self.id,
            "session_id": self.session_id,
            "timestamp": self.timestamp.isoformat(),
            "app_name": self.app_name,
            "pid": self.pid,
            "power_watts": self.power_watts,
            "energy_joules": self.energy_joules,
            "cpu_percent": self.cpu_percent,
            "memory_mb": self.memory_mb,
            "gpu_percent": self.gpu_percent,
            "disk_read_mb": self.disk_read_mb,
            "disk_write_mb": self.disk_write_mb,
            "network_sent_mb": self.network_sent_mb,
            "network_recv_mb": self.network_recv_mb,
            "category": self.category,
            "is_background": self.is_background,
        }


class EnergyRating(db.Model):
    """Aggregated energy rating for an application across sessions."""
    __tablename__ = "energy_ratings"

    id = db.Column(db.Integer, primary_key=True)
    app_name = db.Column(db.String(255), nullable=False, unique=True, index=True)
    category = db.Column(db.String(100), nullable=True)

    total_energy_joules = db.Column(db.Float, default=0.0)
    avg_power_watts = db.Column(db.Float, default=0.0)
    avg_cpu_percent = db.Column(db.Float, default=0.0)
    avg_memory_mb = db.Column(db.Float, default=0.0)
    avg_gpu_percent = db.Column(db.Float, default=0.0)
    avg_disk_read_mb = db.Column(db.Float, default=0.0)
    avg_disk_write_mb = db.Column(db.Float, default=0.0)
    total_monitoring_seconds = db.Column(db.Float, default=0.0)
    sample_count = db.Column(db.Integer, default=0)

    # Rating from A (best) to F (worst) (These are not real ratings, it's more a comparative type of thing that I thought looked nice)
    rating = db.Column(db.String(1), nullable=True)
    last_updated = db.Column(db.DateTime, default=datetime.utcnow)

    def to_dict(self):
        return {
            "id": self.id,
            "app_name": self.app_name,
            "category": self.category,
            "total_energy_joules": self.total_energy_joules,
            "avg_power_watts": self.avg_power_watts,
            "avg_cpu_percent": self.avg_cpu_percent,
            "avg_memory_mb": self.avg_memory_mb,
            "avg_gpu_percent": self.avg_gpu_percent,
            "avg_disk_read_mb": self.avg_disk_read_mb,
            "avg_disk_write_mb": self.avg_disk_write_mb,
            "total_monitoring_seconds": self.total_monitoring_seconds,
            "sample_count": self.sample_count,
            "rating": self.rating,
            "last_updated": self.last_updated.isoformat() if self.last_updated else None,
        }
