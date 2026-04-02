"""Business logic for energy rating computation."""
import re
from collections import defaultdict
from datetime import datetime

from sqlalchemy import func

from app import db
from app.models import EnergySample, EnergyRating

_HELPER_RE = re.compile(
    r"\s*\((?:Renderer|GPU|Plugin|Helper|Notification|Extension|XPC|Agent|"
    r"NetworkExtension|Sandbox|Web Content|Worker)\)[^)]*$"
    r"|\s+Helper(?:\s+\([^)]+\))?$"
    r"|\s+Helper$",
    re.IGNORECASE,
)


def root_app(name: str) -> str:
    """Strip helper/renderer suffixes to find the root application name."""
    stripped = _HELPER_RE.sub("", name).strip()
    return stripped if stripped else name


def compute_ratings():
    """
    Recompute energy ratings for **grouped applications** (not individual
    helper processes).  Samples from e.g. "Discord Helper (Renderer)" are
    folded into the parent "Discord" rating.
    """
    per_process = (
        db.session.query(
            EnergySample.app_name,
            EnergySample.category,
            func.count(EnergySample.id).label("sample_count"),
            func.avg(EnergySample.power_watts).label("avg_power"),
            func.sum(EnergySample.energy_joules).label("total_energy"),
            func.avg(EnergySample.cpu_percent).label("avg_cpu"),
            func.avg(EnergySample.memory_mb).label("avg_memory"),
            func.avg(EnergySample.gpu_percent).label("avg_gpu"),
            func.avg(EnergySample.disk_read_mb).label("avg_disk_read"),
            func.avg(EnergySample.disk_write_mb).label("avg_disk_write"),
        )
        .group_by(EnergySample.app_name, EnergySample.category)
        .all()
    )

    if not per_process:
        return

    groups: dict[str, dict] = {}
    for r in per_process:
        rname = root_app(r.app_name)
        sc = r.sample_count or 1
        if rname not in groups:
            groups[rname] = {
                "category": r.category,
                "sample_count": 0,
                "total_energy": 0.0,
                "_pw": 0.0,
                "_cw": 0.0,
                "_mw": 0.0,
                "_gw": 0.0,
                "_drw": 0.0,
                "_dww": 0.0,
            }
        g = groups[rname]
        g["sample_count"] += sc
        g["total_energy"] += (r.total_energy or 0.0)
        g["_pw"]  += (r.avg_power or 0.0) * sc
        g["_cw"]  += (r.avg_cpu or 0.0) * sc
        g["_mw"]  += (r.avg_memory or 0.0) * sc
        g["_gw"]  += (r.avg_gpu or 0.0) * sc
        g["_drw"] += (r.avg_disk_read or 0.0) * sc
        g["_dww"] += (r.avg_disk_write or 0.0) * sc

    top_names = sorted(groups, key=lambda n: groups[n]["total_energy"], reverse=True)[:40]
    groups = {n: groups[n] for n in top_names}

    for g in groups.values():
        sc = g["sample_count"] or 1
        g["avg_power"]      = g["_pw"]  / sc
        g["avg_cpu"]        = g["_cw"]  / sc
        g["avg_memory"]     = g["_mw"]  / sc
        g["avg_gpu"]        = g["_gw"]  / sc
        g["avg_disk_read"]  = g["_drw"] / sc
        g["avg_disk_write"] = g["_dww"] / sc

    avg_powers = sorted(g["avg_power"] for g in groups.values() if g["avg_power"])
    if not avg_powers:
        return
    n = len(avg_powers)

    def percentile(pct):
        idx = int(n * pct / 100)
        return avg_powers[min(idx, n - 1)]

    thresholds = {
        "A": percentile(20),
        "B": percentile(40),
        "C": percentile(60),
        "D": percentile(80),
        "E": percentile(95),
    }

    for app_name, g in groups.items():
        avg_p = g["avg_power"]
        if avg_p <= thresholds["A"]:
            grade = "A"
        elif avg_p <= thresholds["B"]:
            grade = "B"
        elif avg_p <= thresholds["C"]:
            grade = "C"
        elif avg_p <= thresholds["D"]:
            grade = "D"
        elif avg_p <= thresholds["E"]:
            grade = "E"
        else:
            grade = "F"

        rating = EnergyRating.query.filter_by(app_name=app_name).first()
        if rating is None:
            rating = EnergyRating(app_name=app_name)
            db.session.add(rating)

        rating.category            = g["category"]
        rating.sample_count        = g["sample_count"]
        rating.avg_power_watts     = round(avg_p, 6)
        rating.total_energy_joules = round(g["total_energy"], 6)
        rating.avg_cpu_percent     = round(g["avg_cpu"], 2)
        rating.avg_memory_mb       = round(g["avg_memory"], 2)
        rating.avg_gpu_percent     = round(g["avg_gpu"], 4)
        rating.avg_disk_read_mb    = round(g["avg_disk_read"], 6)
        rating.avg_disk_write_mb   = round(g["avg_disk_write"], 6)
        rating.rating              = grade
        rating.last_updated        = datetime.utcnow()

    current_apps = set(groups.keys())
    stale = EnergyRating.query.filter(~EnergyRating.app_name.in_(current_apps)).all()
    for s in stale:
        db.session.delete(s)

    db.session.commit()
