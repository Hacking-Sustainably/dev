# greenB – Energy Monitor

A Flask web app for uploading, visualising, and comparing macOS application energy-usage data exported from Apple's **powermetrics** tool.

---

## Requirements

- **Python 3.11+**

---

## Setup & Running

```bash
# 1. Clone / enter the project directory
cd energy-monitor

# 2. Create and activate a virtual environment
python3 -m venv venv
source venv/bin/activate

# 3. Install dependencies
pip install -r requirements.txt

# 4. (Optional) Copy and edit environment variables
cp .env.example .env   # edit DATABASE_URL / SECRET_KEY if needed

# 5. Run the development server
python run.py
```

The app will be available at **<http://localhost:5001>**.

To run in production mode:

```bash
FLASK_ENV=production gunicorn -w 2 -b 0.0.0.0:5001 "run:app"
```

---

## App Icons

Icons are extracted automatically on first use and cached as PNGs in `app/static/icons/`. The extraction method depends on the host OS:

| OS | Method |
| --- | --- |
| **macOS** | Spotlight (`mdfind`) → `.icns` → `sips` |
| **Windows** | Registry lookup → `.exe` resource / `.ico` via Pillow |
| **Linux** | XDG icon theme dirs, `.desktop` files, GTK3 theme lookup |

If an icon can't be found, the Bootstrap icon fallback is used. To force re-extraction, delete the app's cached files in `app/static/icons/` and restart the server.

Optional extras for better Linux SVG support: `pip install cairosvg` (or install `rsvg-convert` / `inkscape` system-wide).

---

## Project Structure

```text
energy-monitor/
├── app/
│   ├── icons.py          # macOS icon extraction logic
│   ├── models.py         # SQLAlchemy models
│   ├── services.py       # Business logic / data processing
│   ├── routes/
│   │   ├── dashboard.py  # Main dashboard & app detail views
│   │   ├── api.py        # JSON API endpoints
│   │   └── export.py     # CSV / JSON export
│   ├── static/
│   │   ├── app_logo.png  # App logo (shown in sidebar)
│   │   ├── favicon.svg   # Browser tab favicon
│   │   └── icons/        # Cached app icons (auto-generated)
│   └── templates/        # Jinja2 HTML templates
├── config.py             # Flask configuration classes
├── run.py                # Entry point
└── requirements.txt
```
