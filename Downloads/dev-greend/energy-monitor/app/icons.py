"""
Cross-platform application icon extractor.

- macOS  : Spotlight (mdfind) + sips
- Windows: registry + shell32 / exe resource via Pillow
- Linux  : XDG icon theme + hicolor fallback

Extracted icons are cached as 64×64 PNGs in static/icons/.
"""

import hashlib
import os
import platform
import subprocess
import logging

logger = logging.getLogger(__name__)

ICON_DIR = os.path.join(os.path.dirname(__file__), "static", "icons")
ICON_SIZE = 64  # px
_SYSTEM = platform.system()  # "Darwin" | "Windows" | "Linux"


def _ensure_icon_dir():
    os.makedirs(ICON_DIR, exist_ok=True)


def _safe_filename(app_name: str) -> str:
    """Deterministic, filesystem-safe filename for a given app name."""
    h = hashlib.md5(app_name.encode()).hexdigest()[:10]
    safe = "".join(c if c.isalnum() or c in "-_" else "_" for c in app_name)
    return f"{safe}_{h}.png"


# ---------------------------------------------------------------------------
# macOS
# ---------------------------------------------------------------------------

def _macos_find_app_bundle(app_name: str) -> str | None:
    queries = [
        f'kMDItemFSName == "{app_name}.app"c',
        f'kMDItemDisplayName == "{app_name}"c',
        f'kMDItemFSName == "*{app_name}*"c && kMDItemContentType == "com.apple.application-bundle"',
    ]
    for query in queries:
        try:
            result = subprocess.run(
                ["mdfind", query],
                capture_output=True, text=True, timeout=5,
            )
            paths = [p.strip() for p in result.stdout.strip().split("\n") if p.strip()]
            for preferred in ["/Applications", "/System/Applications"]:
                for p in paths:
                    if p.startswith(preferred) and p.endswith(".app"):
                        return p
            if paths and paths[0].endswith(".app"):
                return paths[0]
        except (subprocess.TimeoutExpired, FileNotFoundError):
            continue
    return None


def _macos_get_icns_path(app_bundle: str) -> str | None:
    try:
        result = subprocess.run(
            ["defaults", "read", f"{app_bundle}/Contents/Info", "CFBundleIconFile"],
            capture_output=True, text=True, timeout=3,
        )
        icon_name = result.stdout.strip()
        if icon_name:
            if not icon_name.endswith(".icns"):
                icon_name += ".icns"
            icns_path = os.path.join(app_bundle, "Contents", "Resources", icon_name)
            if os.path.isfile(icns_path):
                return icns_path
    except (subprocess.TimeoutExpired, FileNotFoundError):
        pass

    resources = os.path.join(app_bundle, "Contents", "Resources")
    if os.path.isdir(resources):
        for fname in os.listdir(resources):
            if fname.endswith(".icns"):
                return os.path.join(resources, fname)
    return None


def _macos_icns_to_png(icns_path: str, output_path: str) -> bool:
    try:
        subprocess.run(
            ["sips", "-s", "format", "png", "-z", str(ICON_SIZE), str(ICON_SIZE),
             icns_path, "--out", output_path],
            capture_output=True, timeout=10,
        )
        return os.path.isfile(output_path) and os.path.getsize(output_path) > 0
    except (subprocess.TimeoutExpired, FileNotFoundError):
        return False


def _get_icon_macos(app_name: str, output_path: str) -> bool:
    bundle = _macos_find_app_bundle(app_name)
    if not bundle:
        return False
    icns = _macos_get_icns_path(bundle)
    if not icns:
        return False
    return _macos_icns_to_png(icns, output_path)


# ---------------------------------------------------------------------------
# Windows
# ---------------------------------------------------------------------------

def _get_icon_windows(app_name: str, output_path: str) -> bool:
    """
    Try to find an app's executable via the registry and extract its icon
    using the Win32 shell API (via Pillow's ImageGrab or win32api if available,
    otherwise fall back to extracting the embedded .ico with Pillow).
    """
    exe_path = _windows_find_exe(app_name)
    if not exe_path or not os.path.isfile(exe_path):
        return False
    return _windows_exe_to_png(exe_path, output_path)


def _windows_find_exe(app_name: str) -> str | None:
    """Search common registry locations for an installed application executable."""
    try:
        import winreg  # only available on Windows
    except ImportError:
        return None

    search_name = app_name.lower()
    reg_paths = [
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths",
        r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\App Paths",
    ]
    for hive in (winreg.HKEY_LOCAL_MACHINE, winreg.HKEY_CURRENT_USER):
        for reg_path in reg_paths:
            try:
                with winreg.OpenKey(hive, reg_path) as base:
                    i = 0
                    while True:
                        try:
                            key_name = winreg.EnumKey(base, i)
                            i += 1
                            if search_name in key_name.lower():
                                with winreg.OpenKey(base, key_name) as sub:
                                    exe, _ = winreg.QueryValueEx(sub, "")
                                    if exe and os.path.isfile(exe):
                                        return exe
                        except OSError:
                            break
            except OSError:
                continue

    # Fallback: walk common install directories
    search_dirs = [
        os.environ.get("PROGRAMFILES", r"C:\Program Files"),
        os.environ.get("PROGRAMFILES(X86)", r"C:\Program Files (x86)"),
        os.path.expanduser("~/AppData/Local"),
    ]
    for base_dir in search_dirs:
        if not base_dir or not os.path.isdir(base_dir):
            continue
        for entry in os.scandir(base_dir):
            if entry.is_dir() and search_name in entry.name.lower():
                for fname in os.listdir(entry.path):
                    if fname.lower().endswith(".exe") and search_name in fname.lower():
                        return os.path.join(entry.path, fname)
    return None


def _windows_exe_to_png(exe_path: str, output_path: str) -> bool:
    """Extract the first icon from a Windows executable and save as PNG."""
    try:
        # Try win32api / win32ui (pywin32) if installed
        import win32ui
        import win32api
        import win32con
        from PIL import Image
        import ctypes

        large, small = ctypes.windll.shell32.ExtractIconExW(exe_path, 0, None, None, 0), None
        ico_x = ctypes.windll.user32.GetSystemMetrics(win32con.SM_CXICON)
        ico_y = ctypes.windll.user32.GetSystemMetrics(win32con.SM_CYICON)
        large_icons = (ctypes.c_void_p * 1)()
        ctypes.windll.shell32.ExtractIconExW(exe_path, 0, large_icons, None, 1)
        hicon = large_icons[0]
        if not hicon:
            return False

        hdc = win32ui.CreateDCFromHandle(win32ui.GetDC(0).GetSafeHdc())
        hbmp = win32ui.CreateBitmap()
        hbmp.CreateCompatibleBitmap(hdc, ico_x, ico_y)
        hdc = hdc.CreateCompatibleDC()
        hdc.SelectObject(hbmp)
        hdc.DrawIcon((0, 0), hicon)
        bmp_info = hbmp.GetInfo()
        bmp_str = hbmp.GetBitmapBits(True)
        img = Image.frombuffer("RGB", (bmp_info["bmWidth"], bmp_info["bmHeight"]), bmp_str, "raw", "BGRX", 0, 1)
        img = img.resize((ICON_SIZE, ICON_SIZE), Image.LANCZOS)
        img.save(output_path, "PNG")
        ctypes.windll.user32.DestroyIcon(hicon)
        return os.path.getsize(output_path) > 0
    except Exception:
        pass

    # Fallback: use Pillow's IcoImagePlugin on .ico files next to the exe
    try:
        from PIL import Image
        exe_dir = os.path.dirname(exe_path)
        for fname in os.listdir(exe_dir):
            if fname.lower().endswith(".ico"):
                ico_path = os.path.join(exe_dir, fname)
                img = Image.open(ico_path)
                img = img.resize((ICON_SIZE, ICON_SIZE), Image.LANCZOS)
                img.save(output_path, "PNG")
                return os.path.getsize(output_path) > 0
    except Exception:
        pass

    return False


# ---------------------------------------------------------------------------
# Linux
# ---------------------------------------------------------------------------

def _get_icon_linux(app_name: str, output_path: str) -> bool:
    """
    Search XDG icon theme directories and .desktop files for an app icon,
    then convert it (PNG / SVG) to a sized PNG with Pillow.
    """
    icon_file = _linux_find_icon(app_name)
    if not icon_file:
        return False
    return _linux_icon_to_png(icon_file, output_path)


def _linux_find_icon(app_name: str) -> str | None:
    search_name = app_name.lower().replace(" ", "-")

    # 1. Try gtk-update-icon-cache / xdg-icon-resource lookup via GTK (if available)
    icon_file = _linux_gtk_lookup(search_name)
    if icon_file:
        return icon_file

    # 2. Walk XDG icon theme directories
    xdg_dirs = [
        os.path.expanduser("~/.local/share/icons"),
        "/usr/share/icons",
        "/usr/share/pixmaps",
        "/usr/local/share/icons",
    ]
    preferred_sizes = [str(ICON_SIZE), "48", "64", "128", "256", "scalable"]
    for base in xdg_dirs:
        if not os.path.isdir(base):
            continue
        for root, _dirs, files in os.walk(base):
            for fname in files:
                name_no_ext = os.path.splitext(fname)[0].lower()
                if name_no_ext == search_name and fname.endswith((".png", ".svg", ".xpm")):
                    # Prefer larger / preferred sizes
                    for size in preferred_sizes:
                        if size in root:
                            return os.path.join(root, fname)
                    return os.path.join(root, fname)

    # 3. Parse .desktop files for Icon= field
    desktop_dirs = [
        os.path.expanduser("~/.local/share/applications"),
        "/usr/share/applications",
        "/usr/local/share/applications",
    ]
    for ddir in desktop_dirs:
        if not os.path.isdir(ddir):
            continue
        for fname in os.listdir(ddir):
            if not fname.endswith(".desktop"):
                continue
            try:
                with open(os.path.join(ddir, fname)) as f:
                    for line in f:
                        if line.lower().startswith("name=") and search_name in line.lower():
                            # Re-read for Icon=
                            pass
                        if line.lower().startswith("icon="):
                            icon_name = line.split("=", 1)[1].strip()
                            if os.path.isabs(icon_name) and os.path.isfile(icon_name):
                                return icon_name
                            # Try to resolve as theme icon name
                            for base in xdg_dirs:
                                for root, _d, files in os.walk(base):
                                    for f2 in files:
                                        if os.path.splitext(f2)[0] == icon_name:
                                            return os.path.join(root, f2)
            except OSError:
                continue
    return None


def _linux_gtk_lookup(icon_name: str) -> str | None:
    """Use GTK3 via gi if available to do a proper theme lookup."""
    try:
        import gi
        gi.require_version("Gtk", "3.0")
        from gi.repository import Gtk
        theme = Gtk.IconTheme.get_default()
        info = theme.lookup_icon(icon_name, ICON_SIZE, 0)
        if info:
            return info.get_filename()
    except Exception:
        pass
    return None


def _linux_icon_to_png(icon_file: str, output_path: str) -> bool:
    ext = os.path.splitext(icon_file)[1].lower()
    try:
        if ext == ".svg":
            # Try cairosvg first, then rsvg-convert, then Inkscape
            return (_svg_via_cairosvg(icon_file, output_path)
                    or _svg_via_subprocess(icon_file, output_path))
        else:
            from PIL import Image
            img = Image.open(icon_file).convert("RGBA")
            img = img.resize((ICON_SIZE, ICON_SIZE), Image.LANCZOS)
            img.save(output_path, "PNG")
            return os.path.getsize(output_path) > 0
    except Exception:
        return False


def _svg_via_cairosvg(svg_path: str, output_path: str) -> bool:
    try:
        import cairosvg
        cairosvg.svg2png(url=svg_path, write_to=output_path,
                         output_width=ICON_SIZE, output_height=ICON_SIZE)
        return os.path.isfile(output_path) and os.path.getsize(output_path) > 0
    except Exception:
        return False


def _svg_via_subprocess(svg_path: str, output_path: str) -> bool:
    for cmd in (
        ["rsvg-convert", "-w", str(ICON_SIZE), "-h", str(ICON_SIZE), svg_path, "-o", output_path],
        ["inkscape", svg_path, f"--export-png={output_path}",
         f"--export-width={ICON_SIZE}", f"--export-height={ICON_SIZE}"],
    ):
        try:
            subprocess.run(cmd, capture_output=True, timeout=10)
            if os.path.isfile(output_path) and os.path.getsize(output_path) > 0:
                return True
        except (subprocess.TimeoutExpired, FileNotFoundError):
            continue
    return False


# ---------------------------------------------------------------------------
# Public API
# ---------------------------------------------------------------------------

def get_icon_path(app_name: str) -> str | None:
    """
    Return the filesystem path to a cached PNG icon for the given app name,
    or None if extraction failed (caller should use fallback).
    """
    _ensure_icon_dir()
    png_name = _safe_filename(app_name)
    png_path = os.path.join(ICON_DIR, png_name)

    if os.path.isfile(png_path):
        return png_path

    # Sentinel: don't retry endlessly for apps whose icon can't be found.
    fail_sentinel = png_path + ".failed"
    if os.path.isfile(fail_sentinel):
        return None

    extractor = {
        "Darwin": _get_icon_macos,
        "Windows": _get_icon_windows,
        "Linux": _get_icon_linux,
    }.get(_SYSTEM)

    if extractor is None:
        return None

    success = False
    try:
        success = extractor(app_name, png_path)
    except Exception:
        logger.exception("Icon extraction failed for %s", app_name)

    if success:
        logger.info("Cached icon for %s → %s", app_name, png_name)
        return png_path

    _mark_failed(fail_sentinel)
    return None


def _mark_failed(sentinel_path: str):
    try:
        with open(sentinel_path, "w") as f:
            f.write("")
    except OSError:
        pass
