import csv
import io
from collections import defaultdict
from datetime import datetime, timedelta

from flask import Blueprint, request, jsonify
from app import db
from app.models import MonitoringSession, EnergySample, EnergyRating
from app.services import compute_ratings

api_bp = Blueprint("api", __name__)


# ---------------------------------------------------------------------------
#  Sessions
# ---------------------------------------------------------------------------

@api_bp.route("/sessions", methods=["GET"])
def list_sessions():
    sessions = MonitoringSession.query.order_by(
        MonitoringSession.created_at.desc()
    ).all()
    return jsonify([s.to_dict() for s in sessions])


@api_bp.route("/sessions/<int:session_id>", methods=["GET"])
def get_session(session_id):
    session = MonitoringSession.query.get_or_404(session_id)
    return jsonify(session.to_dict())


@api_bp.route("/sessions/<int:session_id>", methods=["DELETE"])
def delete_session(session_id):
    session = MonitoringSession.query.get_or_404(session_id)
    db.session.delete(session)
    db.session.commit()
    compute_ratings()
    return jsonify({"message": "Session deleted"}), 200


# ---------------------------------

# ------------------------------------------
#  Upload / Ingest
# ---------------------------------------------------------------------------

@api_bp.route("/upload/json", methods=["POST"])
def upload_json():
    """
    Accept a JSON payload with monitoring data.
    Expected format:
    {
      "session": { "name": "...", "device_name": "...", ... },
      "samples": [
        { "timestamp": "ISO8601", "app_name": "...", "power_watts": ..., ... },
        ...
      ]
    }
    """
    data = request.get_json(force=True)
    if not data:
        return jsonify({"error": "No JSON payload provided"}), 400

    session_info = data.get("session", {})
    samples_data = data.get("samples", [])

    if not samples_data:
        return jsonify({"error": "No samples provided"}), 400

    session = _create_session(session_info, samples_data)
    _ingest_samples(session, samples_data)
    compute_ratings()

    return jsonify({
        "message": f"Imported {len(samples_data)} samples",
        "session": session.to_dict(),
    }), 201


@api_bp.route("/upload/csv", methods=["POST"])
def upload_csv():
    """
    Accept a CSV file upload with energy samples.
    Required columns: timestamp, app_name
    Optional columns: power_watts, energy_joules, cpu_percent, memory_mb,
                      gpu_percent, disk_read_mb, disk_write_mb,
                      network_sent_mb, network_recv_mb, pid, category,
                      is_background
    """
    if "file" not in request.files:
        return jsonify({"error": "No file uploaded"}), 400

    file = request.files["file"]
    if file.filename == "":
        return jsonify({"error": "Empty filename"}), 400

    stream = io.StringIO(file.stream.read().decode("utf-8-sig"))
    reader = csv.DictReader(stream)
    rows = list(reader)

    if not rows:
        return jsonify({"error": "CSV file is empty"}), 400

    session_name = request.form.get("session_name", file.filename)
    session_info = {
        "name": session_name,
        "device_name": request.form.get("device_name"),
        "os_name": request.form.get("os_name"),
        "os_version": request.form.get("os_version"),
    }

    samples_data = []
    for row in rows:
        sample = {}
        for key, value in row.items():
            key = key.strip().lower()
            if value is not None:
                value = value.strip()
            sample[key] = value
        samples_data.append(sample)

    session = _create_session(session_info, samples_data)
    _ingest_samples(session, samples_data)
    compute_ratings()

    return jsonify({
        "message": f"Imported {len(samples_data)} samples from CSV",
        "session": session.to_dict(),
    }), 201


# ---------------------------------------------------------------------------
#  Samples query
# ---------------------------------------------------------------------------

@api_bp.route("/samples", methods=["GET"])
def query_samples():
    """Query samples with optional filters: session_id, app_name, start, end."""
    query = EnergySample.query

    session_id = request.args.get("session_id", type=int)
    app_name = request.args.get("app_name")
    start = request.args.get("start")
    end = request.args.get("end")

    if session_id:
        query = query.filter_by(session_id=session_id)
    if app_name:
        query = query.filter(EnergySample.app_name.ilike(f"%{app_name}%"))
    if start:
        query = query.filter(EnergySample.timestamp >= datetime.fromisoformat(start))
    if end:
        query = query.filter(EnergySample.timestamp <= datetime.fromisoformat(end))

    query = query.order_by(EnergySample.timestamp.asc())
    samples = query.limit(10000).all()
    return jsonify([s.to_dict() for s in samples])


# ---------------------------------------------------------------------------
#  Ratings
# ---------------------------------------------------------------------------

@api_bp.route("/ratings", methods=["GET"])
def get_ratings():
    ratings = EnergyRating.query.order_by(EnergyRating.rating.asc()).all()
    return jsonify([r.to_dict() for r in ratings])


@api_bp.route("/ratings/recompute", methods=["POST"])
def recompute_ratings():
    compute_ratings()
    return jsonify({"message": "Ratings recomputed"})


# ---------------------------------------------------------------------------
#  Summary / analytics helpers
# ---------------------------------------------------------------------------

@api_bp.route("/summary/apps", methods=["GET"])
def app_summary():
    """Return per-app aggregate statistics across all sessions (or one)."""
    session_id = request.args.get("session_id", type=int)
    query = EnergySample.query
    if session_id:
        query = query.filter_by(session_id=session_id)

    from sqlalchemy import func
    results = (
        query
        .with_entities(
            EnergySample.app_name,
            func.count(EnergySample.id).label("sample_count"),
            func.avg(EnergySample.power_watts).label("avg_power"),
            func.sum(EnergySample.energy_joules).label("total_energy"),
            func.avg(EnergySample.cpu_percent).label("avg_cpu"),
            func.avg(EnergySample.memory_mb).label("avg_memory"),
        )
        .group_by(EnergySample.app_name)
        .order_by(func.sum(EnergySample.energy_joules).desc())
        .all()
    )

    return jsonify([
        {
            "app_name": r.app_name,
            "sample_count": r.sample_count,
            "avg_power_watts": round(r.avg_power or 0, 3),
            "total_energy_joules": round(r.total_energy or 0, 3),
            "avg_cpu_percent": round(r.avg_cpu or 0, 2),
            "avg_memory_mb": round(r.avg_memory or 0, 2),
        }
        for r in results
    ])


@api_bp.route("/summary/timeline", methods=["GET"])
def timeline_summary():
    """
    Return power/cpu/memory over time, resampled into at most MAX_POINTS
    evenly-spaced time buckets so charts always look smooth regardless of
    how densely the data was collected.
    """
    MAX_POINTS = 120

    session_id = request.args.get("session_id", type=int)
    app_name = request.args.get("app_name")

    query = EnergySample.query
    if session_id:
        query = query.filter_by(session_id=session_id)
    if app_name:
        query = query.filter(EnergySample.app_name.ilike(f"%{app_name}%"))

    samples = query.order_by(EnergySample.timestamp.asc()).all()

    if not samples:
        return jsonify([])

    # Group raw samples by app_name, then resample each app independently
    # so every app in a stacked chart has the same bucket boundaries.

    # Determine global time range
    t_min = samples[0].timestamp
    t_max = samples[-1].timestamp
    span = (t_max - t_min).total_seconds()

    # If span is tiny or data is already sparse, return as-is
    if span <= 0 or len(samples) <= MAX_POINTS:
        return jsonify([
            {
                "timestamp": s.timestamp.isoformat(),
                "app_name": s.app_name,
                "power_watts": s.power_watts,
                "cpu_percent": s.cpu_percent,
                "memory_mb": s.memory_mb,
            }
            for s in samples
        ])

    bucket_size = span / MAX_POINTS  # seconds per bucket

    # Accumulate samples into buckets per app
    # bucket key = (app_name, bucket_index)
    buckets: dict = defaultdict(lambda: {"power": [], "cpu": [], "mem": []})

    for s in samples:
        offset = (s.timestamp - t_min).total_seconds()
        idx = min(int(offset / bucket_size), MAX_POINTS - 1)
        key = (s.app_name, idx)
        if s.power_watts is not None:
            buckets[key]["power"].append(s.power_watts)
        if s.cpu_percent is not None:
            buckets[key]["cpu"].append(s.cpu_percent)
        if s.memory_mb is not None:
            buckets[key]["mem"].append(s.memory_mb)

    # Collect all app names present
    app_names = sorted({k[0] for k in buckets})

    timeline = []
    for app in app_names:
        for idx in range(MAX_POINTS):
            key = (app, idx)
            if key not in buckets:
                continue
            b = buckets[key]
            bucket_time = t_min + timedelta(seconds=idx * bucket_size + bucket_size / 2)
            timeline.append({
                "timestamp": bucket_time.isoformat(),
                "app_name": app,
                "power_watts": round(sum(b["power"]) / len(b["power"]), 4) if b["power"] else None,
                "cpu_percent": round(sum(b["cpu"]) / len(b["cpu"]), 2) if b["cpu"] else None,
                "memory_mb": round(sum(b["mem"]) / len(b["mem"]), 2) if b["mem"] else None,
            })

    # Sort by timestamp then app so charts render correctly
    timeline.sort(key=lambda x: (x["timestamp"], x["app_name"]))
    return jsonify(timeline)


@api_bp.route("/summary/energy-over-time", methods=["GET"])
def energy_over_time():
    """
    Return cumulative energy (J) over time, bucketed into up to MAX_POINTS
    time intervals.  Returns [{timestamp, energy_joules, cumulative_joules}].
    """
    MAX_POINTS = 100

    samples = (
        EnergySample.query
        .filter(EnergySample.energy_joules.isnot(None))
        .order_by(EnergySample.timestamp.asc())
        .all()
    )

    if not samples:
        return jsonify([])

    t_min = samples[0].timestamp
    t_max = samples[-1].timestamp
    span = (t_max - t_min).total_seconds()

    if span <= 0 or len(samples) <= MAX_POINTS:
        cum = 0.0
        result = []
        for s in samples:
            cum += (s.energy_joules or 0)
            result.append({
                "timestamp": s.timestamp.isoformat(),
                "energy_joules": round(s.energy_joules or 0, 4),
                "cumulative_joules": round(cum, 4),
            })
        return jsonify(result)

    bucket_size = span / MAX_POINTS
    buckets = [0.0] * MAX_POINTS
    counts = [0] * MAX_POINTS

    for s in samples:
        offset = (s.timestamp - t_min).total_seconds()
        idx = min(int(offset / bucket_size), MAX_POINTS - 1)
        buckets[idx] += (s.energy_joules or 0)
        counts[idx] += 1

    cum = 0.0
    result = []
    for idx in range(MAX_POINTS):
        if counts[idx] == 0:
            continue
        cum += buckets[idx]
        bucket_time = t_min + timedelta(seconds=idx * bucket_size + bucket_size / 2)
        result.append({
            "timestamp": bucket_time.isoformat(),
            "energy_joules": round(buckets[idx], 4),
            "cumulative_joules": round(cum, 4),
        })

    return jsonify(result)


# ---------------------------------------------------------------------------
#  Helpers
# ---------------------------------------------------------------------------

def _create_session(session_info, samples_data):
    timestamps = []
    for s in samples_data:
        ts = s.get("timestamp")
        if ts:
            try:
                timestamps.append(datetime.fromisoformat(str(ts)))
            except (ValueError, TypeError):
                pass

    started = min(timestamps) if timestamps else datetime.utcnow()
    ended = max(timestamps) if timestamps else None

    session = MonitoringSession(
        name=session_info.get("name", "Uploaded Session"),
        device_name=session_info.get("device_name"),
        os_name=session_info.get("os_name"),
        os_version=session_info.get("os_version"),
        started_at=started,
        ended_at=ended,
    )
    db.session.add(session)
    db.session.commit()
    return session


def _safe_float(val):
    if val is None or val == "":
        return None
    try:
        return float(val)
    except (ValueError, TypeError):
        return None


def _safe_int(val):
    if val is None or val == "":
        return None
    try:
        return int(val)
    except (ValueError, TypeError):
        return None


def _ingest_samples(session, samples_data):
    objects = []
    for s in samples_data:
        ts_raw = s.get("timestamp")
        try:
            ts = datetime.fromisoformat(str(ts_raw))
        except (ValueError, TypeError):
            ts = datetime.utcnow()

        is_bg = s.get("is_background")
        if isinstance(is_bg, str):
            is_bg = is_bg.lower() in ("true", "1", "yes")

        sample = EnergySample(
            session_id=session.id,
            timestamp=ts,
            app_name=str(s.get("app_name", "Unknown")),
            pid=_safe_int(s.get("pid")),
            power_watts=_safe_float(s.get("power_watts")),
            energy_joules=_safe_float(s.get("energy_joules")),
            cpu_percent=_safe_float(s.get("cpu_percent")),
            memory_mb=_safe_float(s.get("memory_mb")),
            gpu_percent=_safe_float(s.get("gpu_percent")),
            disk_read_mb=_safe_float(s.get("disk_read_mb")),
            disk_write_mb=_safe_float(s.get("disk_write_mb")),
            network_sent_mb=_safe_float(s.get("network_sent_mb")),
            network_recv_mb=_safe_float(s.get("network_recv_mb")),
            category=s.get("category"),
            is_background=bool(is_bg) if is_bg is not None else False,
        )
        objects.append(sample)

    db.session.bulk_save_objects(objects)
    db.session.commit()
