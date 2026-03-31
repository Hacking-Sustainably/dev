from flask import Blueprint, render_template, request, send_file, send_from_directory
from app import db
from app.models import MonitoringSession, EnergySample, EnergyRating
from app.icons import get_icon_path, ICON_DIR
from sqlalchemy import func
import os

dashboard_bp = Blueprint("dashboard", __name__)


@dashboard_bp.route("/icon/<path:app_name>")
def app_icon(app_name):
    """Serve a cached PNG icon for the given application, or the SVG fallback."""
    icon_path = get_icon_path(app_name)
    if icon_path and os.path.isfile(icon_path):
        return send_file(icon_path, mimetype="image/png",
                         max_age=86400)  # cache 1 day
    # Fallback generic icon
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

    # Session filter: ?sessions=1,3
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

    # Top consumers
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
    top_apps = (
        top_q
        .group_by(EnergySample.app_name)
        .order_by(func.sum(EnergySample.energy_joules).desc())
        .limit(15)
        .all()
    )

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

    # Filter ratings if sessions are selected
    filtered_ratings = ratings
    if selected_ids:
        app_names_in_filter = {r.app_name for r in top_apps}
        filtered_ratings = [r for r in ratings if r.app_name in app_names_in_filter]

    return render_template(
        "dashboard.html",
        sessions=all_sessions,
        ratings=filtered_ratings,
        top_apps=top_apps,
        total_samples=total_samples,
        total_energy=round(total_energy, 2),
        all_apps_count=all_apps_count,
        total_avg_power=round(total_avg_power, 2),
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

    return render_template(
        "app_detail.html",
        app_name=app_name,
        samples=samples,
        rating=rating,
    )


@dashboard_bp.route("/upload")
def upload_page():
    """Upload page for importing monitoring data."""
    return render_template("upload.html")


@dashboard_bp.route("/compare")
def compare_page():
    """Compare energy usage across applications."""
    apps = (
        db.session.query(EnergySample.app_name)
        .distinct()
        .order_by(EnergySample.app_name)
        .all()
    )
    app_names = [a[0] for a in apps]
    return render_template("compare.html", app_names=app_names)
