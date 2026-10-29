#!/usr/bin/env python3
"""Test native setup/service CLI with fake sudo, pkexec and systemctl.
No administrator authentication, system configuration, or real service changes.
"""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
ANALYZE = shutil.which("systemd-analyze")
BINARY = Path(os.environ.get("VIMOUSE_BINARY", ROOT / "target/debug/vimouse")).resolve()
if os.geteuid() == 0:
    raise SystemExit("Run setup CLI integration as a regular user; it never requires real root.")

def executable(path, source):
    path.write_text("#!/usr/bin/python3\n" + source)
    path.chmod(0o755)

with tempfile.TemporaryDirectory(prefix="vimouse-setup-test-") as temporary:
    base = Path(temporary)
    home, stubs, runtime = base / "home with % and $", base / "stubs", base / "runtime"
    for p in [home, stubs, runtime]:
        p.mkdir(mode=0o700)
    env = dict(os.environ, HOME=str(home), XDG_CONFIG_HOME=str(home / ".config"),
        XDG_RUNTIME_DIR=str(runtime), PATH=str(stubs), WAYLAND_DISPLAY="wayland-test",
        XDG_CURRENT_DESKTOP="niri", NIRI_SOCKET="/test/niri.sock", VIMOUSE_TEST_SECRET="do-not-import",
        VIMOUSE_TEST_LOG=str(base / "calls.jsonl"))
    helper = '''import json,os,sys
with open(os.environ['VIMOUSE_TEST_LOG'],'a') as f:
    f.write(json.dumps([os.path.basename(sys.argv[0]), *sys.argv[1:]])+'\\n')
sys.exit(int(os.environ.get('VIMOUSE_TEST_AUTH_RESULT','0')))
'''
    executable(stubs / "sudo", helper)
    executable(stubs / "systemctl", '''import json,os,sys
from pathlib import Path
with open(os.environ['VIMOUSE_TEST_LOG'],'a') as f:
    f.write(json.dumps(['systemctl',*sys.argv[1:]])+'\\n')
if os.environ.get('VIMOUSE_TEST_SYSTEMCTL_FAIL') in sys.argv[1:]:
    sys.exit(1)
if 'show' in sys.argv[1:]:
    unit = sys.argv[sys.argv.index('show')+1]
    exists = (Path(os.environ['XDG_CONFIG_HOME'])/'systemd/user'/unit).exists()
    print('loaded' if exists or os.environ.get('VIMOUSE_TEST_LOADED') else 'not-found')
sys.exit(0)
''')
    def run(*args, ok=True, extra=None, executable_path=BINARY):
        result = subprocess.run([executable_path,*args], env=dict(env,**(extra or {})),
            capture_output=True,text=True,timeout=8)
        assert (result.returncode == 0) == ok, result.stdout + result.stderr
        return result
    def calls():
        return [json.loads(line) for line in (base / "calls.jsonl").read_text().splitlines()]
    run("init", "--uid", "0", ok=False)
    assert not (base / "calls.jsonl").exists()
    run("init")
    assert calls()[-1] == ["sudo",str(BINARY),"init","--uid",str(os.geteuid())]
    assert "cancelled" in run("init",ok=False,extra={"VIMOUSE_TEST_AUTH_RESULT":"1"}).stderr
    (stubs / "sudo").rename(stubs / "pkexec")
    run("init")
    assert calls()[-1][0] == "pkexec"
    print("PASS authentication handoff, cancellation and pkexec fallback without root")
    run("start",ok=False)
    run("install-service",ok=False,extra={"VIMOUSE_TEST_SYSTEMCTL_FAIL":"show-environment"})
    binary = home / ".local/bin/vimouse"
    assert not binary.exists(), "failed preflight must not install files"
    before = calls()
    run("install", extra={"XDG_CONFIG_HOME":"relative-config-is-irrelevant"})
    assert binary.read_bytes() == BINARY.read_bytes()
    assert calls() == before, "standalone install must not call authentication or systemctl"
    assert not (home / ".config/systemd/user/vimouse.service").exists()
    run("install", executable_path=binary)
    assert binary.read_bytes() == BINARY.read_bytes()
    print("PASS standalone user installation and self-update without systemd or authentication")
    # Model a Cargo installation, including --root and paths with special characters.
    cargo_binary = home / "custom cargo root/bin/vimouse"
    cargo_binary.parent.mkdir(parents=True)
    shutil.copy2(BINARY, cargo_binary)
    binary.unlink()
    run("install-service", executable_path=cargo_binary)
    assert not binary.exists(), "service registration must not create a second executable"
    unit_path = home / ".config/systemd/user/vimouse.service"
    unit = unit_path.read_text()
    assert 'Type=notify' in unit and '--no-hud' not in unit
    assert "home with %% and $" in unit
    assert str(cargo_binary).replace("%", "%%") in unit
    if ANALYZE:
        # Model WantedBy=graphical-session.target and verify the full login graph,
        # so ordering cycles cannot hide behind a standalone unit parse.
        wants = unit_path.parent / "graphical-session.target.wants"
        wants.mkdir()
        (wants / "vimouse.service").symlink_to(unit_path)
        target = unit_path.parent / "vimouse-test-session.target"
        target.write_text("[Unit]\nWants=graphical-session.target\n")
        verify_env = dict(os.environ, SYSTEMD_UNIT_PATH=str(unit_path.parent) + ":")
        verified = subprocess.run([ANALYZE, "--user", "verify", unit_path, target],
            env=verify_env, capture_output=True, text=True, timeout=8)
        assert verified.returncode == 0, verified.stderr
    session_path = runtime / "systemd/user/vimouse.service.d/session.conf"
    session = session_path.read_text()
    assert 'WAYLAND_DISPLAY=wayland-test' in session and 'NIRI_SOCKET=/test/niri.sock' in session
    assert 'VIMOUSE_TEST_SECRET' not in session
    assert not (home / ".config/systemd/user/vimouse.service.d").exists(), "session settings must be transient"
    assert ["systemctl","--user","enable","vimouse.service"] in calls()
    assert ["systemctl","--user","start","vimouse.service"] not in calls(), "install must not start capture"
    # Simulate the former two-unit layout, then reinstall from the installed binary itself.
    legacy = unit_path.with_name("vimouse-hud.service")
    legacy.write_text("[Service]\nExecStart=/bin/false\n")
    run("install-service",executable_path=cargo_binary)
    assert not binary.exists()
    assert ["systemctl","--user","disable","--now","vimouse-hud.service"] in calls()
    print("PASS Cargo executable registration without copying, special paths and legacy migration")
    run("start")
    run("stop")
    run("status")
    for action in ["start","stop"]:
        assert ["systemctl","--user",action,"vimouse.service"] in calls()
    assert ["systemctl","--user","stop","vimouse-hud.service"] in calls()
    assert 'failed' in run("start",ok=False,extra={"VIMOUSE_TEST_SYSTEMCTL_FAIL":"start"}).stderr
    print("PASS start/stop/status and systemctl failure propagation without real service changes")

    # Removal stops/disables before deleting files, and preserves user data/overrides.
    config = home / ".config/vimouse/config.toml"
    marks = home / ".local/state/vimouse/marks.json"
    for path in [config, marks]:
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text("keep user data")
    custom = session_path.with_name("custom.conf")
    custom.write_text("# user override")
    legacy_session = runtime / "systemd/user/vimouse-hud.service.d/session.conf"
    legacy_session.parent.mkdir(parents=True)
    legacy_session.write_text("# managed session")
    wants = unit_path.parent / "graphical-session.target.wants"
    wants.mkdir(exist_ok=True)
    (wants / legacy.name).symlink_to(legacy)
    run("uninstall-service", ok=False, extra={"VIMOUSE_TEST_SYSTEMCTL_FAIL":"disable"})
    assert unit_path.exists() and session_path.exists(), "keep files if stopping/disabling fails"
    before = len(calls())
    run("uninstall-service")
    removed = calls()[before:]
    for name in ["vimouse.service", "vimouse-hud.service"]:
        assert ["systemctl", "--user", "disable", "--now", name] in removed
        assert not (unit_path.parent / name).exists()
        assert not (wants / name).is_symlink()
    assert not session_path.exists() and not legacy_session.exists()
    assert custom.read_text() == "# user override"
    assert config.read_text() == marks.read_text() == "keep user data"
    assert cargo_binary.exists(), "Cargo owns the executable"
    before = len(calls())
    run("uninstall-service")
    assert not any("disable" in call for call in calls()[before:]), "repeated uninstall is safe"
    run("uninstall-service", extra={"VIMOUSE_TEST_LOADED":"1"})
    print("PASS idempotent service removal, loaded orphan cleanup, failure handling and user data preservation")
