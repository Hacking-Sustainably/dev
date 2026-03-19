"""Business logic for energy rating computation."""
from datetime import datetime
from app import db
from app.models import EnergySample, EnergyRating
from sqlalchemy import func


def compute_ratings():
    """
    Recompute the energy rating for every application based on all stored samples.
    Assigns a letter grade A-F based on average power consumption relative to peers.
    """
    results = (
        db.session.query(
            EnergySample.app_name,
            EnergySample.category,
            func.count(EnergySample.id).label("sample_count"),
            func.avg(EnergySample.power_watts).label("avg_power"),
            func.sum(EnergySample.energy_joules).label("total_energy"),
            func.avg(EnergySample.cpu_percent).label("avg_cpu"),
            func.avg(EnergySample.memory_mb).label("avg_memory"),
        )
        .group_by(EnergySample.app_name, EnergySample.category)
        .all()
    )

    if not results:
        return

    # Determine percentile thresholds from average power across all apps
    avg_powers = [r.avg_power for r in results if r.avg_power is not None]
    if not avg_powers:
        return

    avg_powers_sorted = sorted(avg_powers)
    n = len(avg_powers_sorted)

    def percentile(pct):
        idx = int(n * pct / 100)
        return avg_powers_sorted[min(idx, n - 1)]

    thresholds = {
        "A": percentile(20),   # bottom 20% power = best
        "B": percentile(40),
        "C": percentile(60),
        "D": percentile(80),
        "E": percentile(95),
        # F = above 95th percentile
    }

    for r in results:
        avg_p = r.avg_power or 0
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

        rating = EnergyRating.query.filter_by(app_name=r.app_name).first()
        if rating is None:
            rating = EnergyRating(app_name=r.app_name)
            db.session.add(rating)

        rating.category = r.category
        rating.sample_count = r.sample_count
        rating.avg_power_watts = round(r.avg_power or 0, 4)
        rating.total_energy_joules = round(r.total_energy or 0, 4)
        rating.avg_cpu_percent = round(r.avg_cpu or 0, 2)
        rating.avg_memory_mb = round(r.avg_memory or 0, 2)
        rating.rating = grade
        rating.last_updated = datetime.utcnow()

    # Remove ratings for apps no longer in samples
    current_apps = {r.app_name for r in results}
    stale = EnergyRating.query.filter(~EnergyRating.app_name.in_(current_apps)).all()
    for s in stale:
        db.session.delete(s)

    db.session.commit()
