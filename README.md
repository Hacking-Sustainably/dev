# `greenb` energy monitoring

to run our application, you can execute the script below:
```bash
if ! uname | grep -q "Darwin"; then
    echo "greenb is currently only supported on macos :("
    exit 2
fi

if [ ! -d "greend" ] || [ ! -d "energy-monitor" ]; then
    echo "$(pwd) is not the project root"
    exit 3
fi

# first build everything
cd greend
cargo build --release
cd ../energy-monitor
python3 -m venv .venv
source .venv/bin/activate
pip3 install -r requirements.txt
cd ..

# in parallel
{
    sudo ./greend/target/release/greend
} &
{
    cd energy-monitor
    python3 run.py
}
```
