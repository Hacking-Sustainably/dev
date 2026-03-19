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
    sessions = MonitoringSession.query.order_by(
        MonitoringSession.created_at.desc()
    ).limit(20).all()
    ratings = EnergyRating.query.order_by(EnergyRating.rating.asc()).all()

    # Top consumers
    top_apps = (
        db.session.query(
            EnergySample.app_name,
            func.avg(EnergySample.power_watts).label("avg_power"),
            func.sum(EnergySample.energy_joules).label("total_energy"),
            func.avg(EnergySample.cpu_percent).label("avg_cpu"),
            func.count(EnergySample.id).label("sample_count"),
        )
        .group_by(EnergySample.app_name)
        .order_by(func.sum(EnergySample.energy_joules).desc())
        .limit(15)
        .all()
    )

    total_samples = EnergySample.query.count()
    total_energy = (
        db.session.query(func.sum(EnergySample.energy_joules)).scalar() or 0
    )
    all_apps_count = (
        db.session.query(func.count(func.distinct(EnergySample.app_name))).scalar() or 0
    )
    total_avg_power = (
        db.session.query(func.avg(EnergySample.power_watts)).scalar() or 0
    )

    return render_template(
        "dashboard.html",
        sessions=sessions,
        ratings=ratings,
        top_apps=top_apps,
        total_samples=total_samples,
        total_energy=round(total_energy, 2),
        all_apps_count=all_apps_count,
        total_avg_power=round(total_avg_power, 2),
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
