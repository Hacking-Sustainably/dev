import re
import os
import subprocess as _subprocess

from flask import Blueprint, render_template, request, send_file, send_from_directory
from sqlalchemy import func

from app import db
from app.icons import get_icon_path, ICON_DIR
from app.models import EnergyRating, EnergySample, MonitoringSession
from app.services import root_app


def _root_app(name: str) -> str:
    return root_app(name)


def _group_apps(flat_rows):
    """Group flat per-process rows under root app names, sorted by total energy."""
    groups: dict[str, dict] = {}
    for r in flat_rows:
        root = _root_app(r.app_name)
        if root not in groups:
            groups[root] = {
                "app_name": root,
                "total_energy": 0.0,
                "avg_power": 0.0,
                "avg_cpu": 0.0,
                "avg_memory": 0.0,
                "avg_gpu": 0.0,
                "avg_disk_read": 0.0,
                "avg_disk_write": 0.0,
                "sample_count": 0,
                "_power_weight": 0.0,
                "_cpu_weight": 0.0,
                "_mem_weight": 0.0,
                "_gpu_weight": 0.0,
                "_dr_weight": 0.0,
                "_dw_weight": 0.0,
                "subprocesses": [],
            }
        g = groups[root]
        e = r.total_energy or 0.0
        sc = r.sample_count or 1
        g["total_energy"] += e
        g["sample_count"] += sc
        g["_power_weight"] += (r.avg_power or 0.0) * sc
        g["_cpu_weight"]   += (r.avg_cpu or 0.0) * sc
        g["_mem_weight"]   += (r.avg_memory or 0.0) * sc
        g["_gpu_weight"]   += (r.avg_gpu or 0.0) * sc
        g["_dr_weight"]    += (r.avg_disk_read or 0.0) * sc
        g["_dw_weight"] += (r.avg_disk_write or 0.0) * sc
        if r.app_name != root:
            g["subprocesses"].append({
                "app_name": r.app_name,
                "total_energy": round(e, 2),
                "avg_power": round(r.avg_power or 0.0, 3),
                "avg_cpu": round(r.avg_cpu or 0.0, 1),
            })

    result = []
    for g in groups.values():
        sc = g["sample_count"] or 1
        g["avg_power"]      = round(g["_power_weight"] / sc, 3)
        g["avg_cpu"]        = round(g["_cpu_weight"]   / sc, 1)
        g["avg_memory"]     = round(g["_mem_weight"]   / sc, 1)
        g["avg_gpu"]        = round(g["_gpu_weight"]   / sc, 1)
        g["avg_disk_read"]  = round(g["_dr_weight"]    / sc, 4)
        g["avg_disk_write"] = round(g["_dw_weight"]    / sc, 4)
        g["total_energy"]   = round(g["total_energy"], 2)
        g["subprocesses"].sort(key=lambda s: s["total_energy"], reverse=True)
        for k in ("_power_weight", "_cpu_weight", "_mem_weight",
                  "_gpu_weight", "_dr_weight", "_dw_weight"):
            del g[k]
        result.append(g)

    result.sort(key=lambda g: g["total_energy"], reverse=True)
    return result


def _resolve_process_path(app_name: str) -> str | None:
    """Try to resolve the on-disk path for a macOS process/app by name."""
    try:
        out = _subprocess.check_output(
            ["mdfind", f"kMDItemDisplayName == '{app_name}'cd"],
            timeout=3, text=True, stderr=_subprocess.DEVNULL,
        ).strip()
        if out:
            first = out.splitlines()[0]
            if first:
                return first
    except Exception:
        pass

    app_path = f"/Applications/{app_name}.app"
    if os.path.isdir(app_path):
        return app_path

    try:
        out = _subprocess.check_output(
            ["which", app_name],
            timeout=2, text=True, stderr=_subprocess.DEVNULL,
        ).strip()
        if out and os.path.isfile(out):
            return out
    except Exception:
        pass

    return None


dashboard_bp = Blueprint("dashboard", __name__)


@dashboard_bp.route("/icon/<path:app_name>")
def app_icon(app_name):
    """Serve a cached PNG icon for the given application, or the SVG fallback."""
    icon_path = get_icon_path(app_name)
    if icon_path and os.path.isfile(icon_path):
        return send_file(icon_path, mimetype="image/png",
                         max_age=86400)
    return send_from_directory(
        os.path.join(os.path.dirname(os.path.dirname(__file__)), "static", "icons"),
        "fallback.svg",
        mimetype="image/svg+xml",
        max_age=86400,
    )


@dashboard_bp.route("/")
def index():
    """Main dashboard with overview."""
    all_sessions = MonitoringSession.query.order_by(
        MonitoringSession.created_at.desc()
    ).limit(20).all()
    ratings = EnergyRating.query.order_by(EnergyRating.rating.asc()).all()

    selected_ids = request.args.get("sessions", "")
    if selected_ids:
        selected_ids = [int(x) for x in selected_ids.split(",") if x.strip().isdigit()]
    else:
        selected_ids = []

    def base_query():
        q = EnergySample.query
        if selected_ids:
            q = q.filter(EnergySample.session_id.in_(selected_ids))
        return q

    top_q = (
        db.session.query(
            EnergySample.app_name,
            func.avg(EnergySample.power_watts).label("avg_power"),
            func.sum(EnergySample.energy_joules).label("total_energy"),
            func.avg(EnergySample.cpu_percent).label("avg_cpu"),
            func.count(EnergySample.id).label("sample_count"),
            func.avg(EnergySample.memory_mb).label("avg_memory"),
            func.avg(EnergySample.gpu_percent).label("avg_gpu"),
            func.avg(EnergySample.disk_read_mb).label("avg_disk_read"),
            func.avg(EnergySample.disk_write_mb).label("avg_disk_write"),
        )
    )
    if selected_ids:
        top_q = top_q.filter(EnergySample.session_id.in_(selected_ids))
    flat_apps = (
        top_q
        .group_by(EnergySample.app_name)
        .order_by(func.sum(EnergySample.energy_joules).desc())
        .all()
    )
    top_apps = _group_apps(flat_apps)[:20]

    total_samples = base_query().count()
    total_energy = (
        base_query().with_entities(func.sum(EnergySample.energy_joules)).scalar() or 0
    )
    all_apps_count = (
        base_query().with_entities(func.count(func.distinct(EnergySample.app_name))).scalar() or 0
    )
    total_avg_power = (
        base_query().with_entities(func.avg(EnergySample.power_watts)).scalar() or 0
    )
    sys_avg_cpu_sub = (
        base_query()
        .with_entities(
            EnergySample.timestamp,
            func.sum(EnergySample.cpu_percent).label("total_cpu"),
        )
        .group_by(EnergySample.timestamp)
        .subquery()
    )
    sys_avg_cpu = (
        db.session.query(func.avg(sys_avg_cpu_sub.c.total_cpu)).scalar() or 0
    )
    sys_avg_disk_read = (
        base_query().with_entities(func.avg(EnergySample.disk_read_mb)).scalar() or 0
    )
    sys_avg_disk_write = (
        base_query().with_entities(func.avg(EnergySample.disk_write_mb)).scalar() or 0
    )
    sys_avg_gpu = (
        base_query().with_entities(func.avg(EnergySample.gpu_percent)).scalar() or 0
    )
    sys_avg_memory = (
        base_query().with_entities(func.avg(EnergySample.memory_mb)).scalar() or 0
    )

    grouped_app_names = {a["app_name"] for a in top_apps}
    top30_names = {g["app_name"] for g in _group_apps(flat_apps)[:30]}
    filtered_ratings = [
        r for r in ratings
        if r.app_name in top30_names
    ]
    if selected_ids:
        filtered_ratings = [
            r for r in filtered_ratings if r.app_name in grouped_app_names
        ]

    return render_template(
        "dashboard.html",
        sessions=all_sessions,
        ratings=filtered_ratings,
        top_apps=top_apps,
        total_samples=total_samples,
        total_energy=round(total_energy, 2),
        all_apps_count=all_apps_count,
        total_avg_power_mw=round(total_avg_power * 1000, 2),
        sys_avg_cpu=round(sys_avg_cpu, 2),
        sys_avg_disk_read=round(sys_avg_disk_read, 4),
        sys_avg_disk_write=round(sys_avg_disk_write, 4),
        sys_avg_gpu=round(sys_avg_gpu, 2),
        sys_avg_memory=round(sys_avg_memory, 1),
        selected_session_ids=selected_ids,
    )


@dashboard_bp.route("/session/<int:session_id>")
def session_detail(session_id):
    """Detailed view for a single monitoring session."""
    session = MonitoringSession.query.get_or_404(session_id)

    per_app = (
        db.session.query(
            EnergySample.app_name,
            func.avg(EnergySample.power_watts).label("avg_power"),
            func.sum(EnergySample.energy_joules).label("total_energy"),
            func.avg(EnergySample.cpu_percent).label("avg_cpu"),
            func.avg(EnergySample.memory_mb).label("avg_memory"),
            func.count(EnergySample.id).label("sample_count"),
            func.avg(EnergySample.gpu_percent).label("avg_gpu"),
            func.avg(EnergySample.disk_read_mb).label("avg_disk_read"),
            func.avg(EnergySample.disk_write_mb).label("avg_disk_write"),
        )
        .filter_by(session_id=session_id)
        .group_by(EnergySample.app_name)
        .order_by(func.sum(EnergySample.energy_joules).desc())
        .all()
    )

    return render_template(
        "session_detail.html",
        session=session,
        per_app=per_app,
    )


@dashboard_bp.route("/app/<path:app_name>")
def app_detail(app_name):
    """Detailed view for a single application across all sessions."""
    samples = (
        EnergySample.query
        .filter(EnergySample.app_name == app_name)
        .order_by(EnergySample.timestamp.asc())
        .all()
    )

    rating = EnergyRating.query.filter_by(app_name=app_name).first()
    process_path = _resolve_process_path(app_name)

    return render_template(
        "app_detail.html",
        app_name=app_name,
        samples=samples,
        rating=rating,
        process_path=process_path,
    )


@dashboard_bp.route("/upload")
def upload_page():
    """Upload page for importing monitoring data."""
    return render_template("upload.html")


@dashboard_bp.route("/compare")
def compare_page():
    """Compare energy usage across applications (grouped, no helpers)."""
    flat_apps = (
        db.session.query(
            EnergySample.app_name,
            func.avg(EnergySample.power_watts).label("avg_power"),
            func.sum(EnergySample.energy_joules).label("total_energy"),
            func.avg(EnergySample.cpu_percent).label("avg_cpu"),
            func.count(EnergySample.id).label("sample_count"),
            func.avg(EnergySample.memory_mb).label("avg_memory"),
            func.avg(EnergySample.gpu_percent).label("avg_gpu"),
            func.avg(EnergySample.disk_read_mb).label("avg_disk_read"),
            func.avg(EnergySample.disk_write_mb).label("avg_disk_write"),
        )
        .group_by(EnergySample.app_name)
        .order_by(func.sum(EnergySample.energy_joules).desc())
        .all()
    )
    grouped = _group_apps(flat_apps)

    grouped = grouped[:40]

    all_ratings = {r.app_name: r.rating for r in EnergyRating.query.all()}
    for g in grouped:
        g["rating"] = all_ratings.get(g["app_name"], None)

    return render_template("compare.html", grouped_apps=grouped)
