import csv
import io
import json
from datetime import datetime

from flask import Blueprint, request, Response
from app import db
from app.models import EnergySample, EnergyRating

export_bp = Blueprint("export", __name__)


@export_bp.route("/csv")
def export_csv():
    """Export samples as CSV. Optional query params: session_id, app_name."""
    session_id = request.args.get("session_id", type=int)
    app_name = request.args.get("app_name")

    query = EnergySample.query
    if session_id:
        query = query.filter_by(session_id=session_id)
    if app_name:
        query = query.filter(EnergySample.app_name.ilike(f"%{app_name}%"))

    samples = query.order_by(EnergySample.timestamp.asc()).all()

    output = io.StringIO()
    writer = csv.writer(output)
    writer.writerow([
        "timestamp", "app_name", "pid", "power_watts", "energy_joules",
        "cpu_percent", "memory_mb", "gpu_percent", "disk_read_mb",
        "disk_write_mb", "network_sent_mb", "network_recv_mb",
        "category", "is_background", "session_id",
    ])
    for s in samples:
        writer.writerow([
            s.timestamp.isoformat(), s.app_name, s.pid, s.power_watts,
            s.energy_joules, s.cpu_percent, s.memory_mb, s.gpu_percent,
            s.disk_read_mb, s.disk_write_mb, s.network_sent_mb,
            s.network_recv_mb, s.category, s.is_background, s.session_id,
        ])

    filename = "energy_samples"
    if session_id:
        filename += f"_session{session_id}"
    if app_name:
        filename += f"_{app_name}"
    filename += ".csv"

    return Response(
        output.getvalue(),
        mimetype="text/csv",
        headers={"Content-Disposition": f"attachment; filename={filename}"},
    )


@export_bp.route("/json")
def export_json():
    """Export samples as JSON."""
    session_id = request.args.get("session_id", type=int)
    app_name = request.args.get("app_name")

    query = EnergySample.query
    if session_id:
        query = query.filter_by(session_id=session_id)
    if app_name:
        query = query.filter(EnergySample.app_name.ilike(f"%{app_name}%"))

    samples = query.order_by(EnergySample.timestamp.asc()).all()

    data = {
        "exported_at": datetime.utcnow().isoformat(),
        "filters": {"session_id": session_id, "app_name": app_name},
        "count": len(samples),
        "samples": [s.to_dict() for s in samples],
    }

    filename = "energy_samples"
    if session_id:
        filename += f"_session{session_id}"
    if app_name:
        filename += f"_{app_name}"
    filename += ".json"

    return Response(
        json.dumps(data, indent=2),
        mimetype="application/json",
        headers={"Content-Disposition": f"attachment; filename={filename}"},
    )


@export_bp.route("/ratings/csv")
def export_ratings_csv():
    """Export application energy ratings as CSV."""
    ratings = EnergyRating.query.order_by(EnergyRating.rating.asc()).all()

    output = io.StringIO()
    writer = csv.writer(output)
    writer.writerow([
        "app_name", "category", "rating", "avg_power_watts",
        "total_energy_joules", "avg_cpu_percent", "avg_memory_mb",
        "sample_count", "total_monitoring_seconds",
    ])
    for r in ratings:
        writer.writerow([
            r.app_name, r.category, r.rating, r.avg_power_watts,
            r.total_energy_joules, r.avg_cpu_percent, r.avg_memory_mb,
            r.sample_count, r.total_monitoring_seconds,
        ])

    return Response(
        output.getvalue(),
        mimetype="text/csv",
        headers={
            "Content-Disposition": "attachment; filename=energy_ratings.csv",
        },
    )
