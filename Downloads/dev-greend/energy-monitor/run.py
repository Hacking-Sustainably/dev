import os
from app import create_app, get_database_path


app = create_app()

# Ensure the instance folder and database exist before serving requests.
# db.create_all() is called inside create_app(), but the instance dir must
# exist first so SQLite can write the file.
instance_dir = os.path.join(os.path.dirname(__file__), "instance")
os.makedirs(instance_dir, exist_ok=True)

with app.app_context():
    from app import db
    db_path = f"sqlite://{get_database_path()}"
    if not os.path.exists(db_path):
        db.create_all()
        print(f"✓ Database created at {db_path}")
    else:
        # Still run create_all so any new tables from model changes are added.
        db.create_all()

if __name__ == "__main__":
    app.run(debug=True, port=5001)
