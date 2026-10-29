#!/usr/bin/env python3
"""Isolated, headless Wayland integration. Never captures input or changes the desktop.
Development-only requirements: KWin, dbus-run-session, Python 3.
"""
import json
import os
from pathlib import Path
import re
import shutil
import signal
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
BINARY = Path(os.environ.get("VIMOUSE_BINARY", ROOT / "target/debug/vimouse")).resolve()
for tool in ["kwin_wayland", "dbus-run-session"]:
    if not shutil.which(tool):
        raise SystemExit(f"Development integration test needs {tool}; it is not a Vimouse runtime dependency.")

with tempfile.TemporaryDirectory(prefix="vimouse-native-test-") as temporary:
    base = Path(temporary)
    base.chmod(0o700)
    env = dict(os.environ, XDG_RUNTIME_DIR=str(base), QT_QPA_PLATFORM="offscreen",
               KWIN_COMPOSE="Q", LIBGL_ALWAYS_SOFTWARE="1")
    server_log = (base / "server.log").open("w")
    server = subprocess.Popen(["dbus-run-session", "--", "kwin_wayland", "--virtual",
        "--no-lockscreen", "--no-global-shortcuts", "--no-kactivities", "--socket", "wayland-test",
        "--width", "1280", "--height", "720", "--output-count", "2"],
        env=env, stdout=server_log, stderr=server_log, start_new_session=True)
    children = []
    try:
        for _ in range(150):
            if (base / "wayland-test").exists():
                break
            if server.poll() is not None:
                raise AssertionError((base / "server.log").read_text())
            time.sleep(.04)
        assert (base / "wayland-test").exists(), "headless compositor startup timed out"
        # An empty PATH proves there is no qml, kscreen-doctor, qdbus, or font helper dependency.
        (base / "empty-path").mkdir()
        env.update(WAYLAND_DISPLAY="wayland-test", XDG_CURRENT_DESKTOP="native-test",
                   PATH=str(base / "empty-path"))
        for name in ["WAYLAND_SOCKET", "NIRI_SOCKET"]:
            env.pop(name, None)
        outputs = json.loads(subprocess.check_output([BINARY, "native-displays"], env=env, timeout=5))
        assert len(outputs) == 2, outputs
        layout = json.loads(subprocess.check_output([BINARY, "displays"], env=env, timeout=5))
        assert layout["source"] == "Wayland", layout
        assert len(layout["layout"]["outputs"]) == 2
        print("PASS native dual-output geometry with empty PATH")
        runtime = base / "runtime"
        log = (base / "client.log").open("w")
        demo = subprocess.Popen([BINARY, "demo", "--runtime-dir", runtime], env=dict(env, WAYLAND_DEBUG="1"),
            stdin=subprocess.PIPE, stdout=(base / "demo.log").open("w"), stderr=log, text=True)
        children.append(demo)
        def wait_status(mode, display=None, columns=None, speed=None):
            deadline = time.monotonic() + 5
            while time.monotonic() < deadline:
                if demo.poll() is not None:
                    raise AssertionError((base / "client.log").read_text()[-4000:])
                try:
                    status = json.loads((runtime / "status.json").read_text())
                    if status["mode"] == mode and (display is None or status["display"]["name"] == display) and (columns is None or status.get("grid", {}).get("columns") == columns) and (speed is None or status["speed"] == speed):
                        time.sleep(.12)  # Give the independent overlay a status polling cycle.
                        return status
                except (OSError, ValueError):
                    pass
                time.sleep(.03)
            raise AssertionError(f"No {mode} status; client log: " + (base / "client.log").read_text()[-2000:])
        def send(keys):
            demo.stdin.write(keys + "\n")
            demo.stdin.flush()
        first = wait_status("insert")
        send("caps 3")
        wait_status("normal", speed=2)
        send("1")
        wait_status("normal", speed=0)
        send("m a space m b M")
        state=wait_status("marks")
        assert len(state["mark_targets"])==2 and all(mark["position"] for mark in state["mark_targets"])
        assert len({mark["display"] for mark in state["mark_targets"]})==2
        send("a ?")
        wait_status("help")
        send("esc f")
        assert wait_status("grid", columns=5)["grid"]["columns"] == 5
        send("m tab")
        assert wait_status("grid", columns=4)["grid"]["columns"] == 4
        send("p")
        assert wait_status("grid", columns=3)["grid"]["columns"] == 3
        send("tab")
        assert wait_status("grid", columns=2)["grid"]["columns"] == 2
        send("space space f")
        other = next(o["name"] for o in outputs if o["name"] != first["display"]["name"])
        wait_status("grid", other)
        send("a i")
        wait_status("insert")
        send("caps : q enter")
        assert demo.wait(timeout=5) == 0
        text = (base / "client.log").read_text()
        assert text.count(".get_layer_surface(") == 2, text[-4000:]
        assert text.count(".set_keyboard_interactivity(0)") == 2
        assert text.count(".set_input_region(") == 2
        assert ".get_keyboard(" not in text and ".get_pointer(" not in text
        assert re.search(r"\.set_buffer_scale\(1\)", text), "buffer scale missing"
        assert not re.search(r"\.attach\((?:nil|null|None)", text), "surfaces must stay mapped"
        assert not re.search(r"protocol error|panicked|Could not start", text, re.I), text[-4000:]
        print("PASS same-binary HUD, numeric speed, multi-output marks, help, f / Tab / Space")
        print("PASS input-transparent surfaces stay mapped; no keyboard/pointer bindings")
        # Fault injection is confined to this private compositor and a no-input demo.
        fault_runtime=base / "fault-runtime"
        failed_demo=subprocess.Popen([BINARY,"demo","--runtime-dir",fault_runtime],env=env,
            stdin=subprocess.PIPE,stdout=(base/"fault.stdout").open("w"),stderr=(base/"fault.stderr").open("w"),text=True)
        children.append(failed_demo)
        for _ in range(150):
            if (fault_runtime/"status.json").exists():break
            assert failed_demo.poll() is None,(base/"fault.stderr").read_text()
            time.sleep(.03)
        descendants=Path(f"/proc/{failed_demo.pid}/task/{failed_demo.pid}/children").read_text().split()
        hud_pid=next(int(pid) for pid in descendants if b"hud" in Path("/proc",pid,"cmdline").read_bytes().split(b"\0"))
        os.kill(hud_pid,signal.SIGTERM)
        assert failed_demo.wait(timeout=3)!=0,"controller must stop if its requested HUD exits"
        assert "HUD exited" in (base/"fault.stderr").read_text()
        assert not (fault_runtime/"status.json").exists()
        print("PASS unexpected HUD exit is reaped and stops the controller while stdin is idle")

    finally:
        for child in children:
            if child.poll() is None:
                child.terminate()
                try:
                    child.wait(timeout=3)
                except subprocess.TimeoutExpired:
                    child.kill()
                    child.wait()
        if server.poll() is None:
            os.killpg(server.pid, signal.SIGTERM)
            try:
                server.wait(timeout=3)
            except subprocess.TimeoutExpired:
                os.killpg(server.pid, signal.SIGKILL)
                server.wait()
