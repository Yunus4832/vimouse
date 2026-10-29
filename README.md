# Vimouse

English | [简体中文](README.zh-CN.md)

Control your mouse with Vim-style keys: move with `hjkl`, jump with `f`, and fine-tune with `Tab`. Press `Caps Lock` to start controlling the mouse; press it again to return to typing.

Vimouse is written in Rust for Linux. Its native Wayland overlay includes embedded fonts and requires no Qt or GTK. A musl build runs as a single static binary.

During mouse control, a small capsule in the bottom-right corner shows the mode and speed. Command mode expands it into an input bar; typing mode hides it. The overlay lets mouse clicks pass through and never takes keyboard focus.

![Mode and speed capsule](docs/hud.png)

![Command input bar](docs/command-hud.png)

## Supported environments

The full interface requires **Linux and a Wayland compositor with layer-shell support**. KDE Plasma Wayland and niri are the primary targets. The current static build target is x86_64.

- Keyboard devices and `/dev/uinput` need appropriate permissions. Run `vimouse init` once to configure access.
- GNOME Wayland and X11 have no overlay backend yet. You can try input control with `--no-hud`, but the visual grids will be unavailable.
- Windows and macOS are not supported. See [compatibility and limitations](docs/guide.md#010-支持环境与限制) (Chinese) for details about other compositors.

## Install and run

Install from GitHub on Linux with the Rust toolchain. Cargo manages the executable:

```sh
cargo install --git https://github.com/Yunus4832/vimouse.git --locked

vimouse init
vimouse doctor
vimouse run
```

Run these commands **as your regular user in a desktop terminal**. `init` requests administrator authentication through sudo or pkexec when needed to configure uinput and device permissions. Daily use does not require sudo. Access to currently connected devices takes effect immediately; log out and back in once so group permissions also apply to future devices and services.

To try the interface first, run `vimouse demo`, type `caps f a`, and press Enter. The demo neither captures your keyboard nor moves the system pointer.

To start Vimouse automatically when you log in, exit the foreground instance and install the user service:

```sh
vimouse install-service
vimouse start
```

`install-service` registers the current executable and enables login autostart without copying the binary. `start` launches the service immediately. Cargo normally installs to `~/.cargo/bin`; ensure its installation directory is on PATH. Custom Cargo installation roots are also supported. See [installation and device permissions](docs/guide.md#安装与设备权限) (Chinese) for details.

## Basic controls

Vimouse starts in **Insert mode**, where you type normally. Press `Caps Lock` to enter **Normal mode** for mouse control. Press `?` in Normal mode to open the shortcut reference.

| Key | Action |
| --- | --- |
| `h` / `j` / `k` / `l` | Hold to move left / down / up / right |
| `1` / `2` / `3` | Select fast / medium / fine speed |
| `Enter` | Left-click |
| `Alt+Enter` | Right-click and enter menu selection mode |
| `Ctrl+Enter` | Middle-click |
| `v` | Start a left-button drag; press again to release |
| `Alt+hjkl` | Scroll horizontally or vertically |
| `Space` | Switch to the next display and center the pointer |
| `i` or `Caps Lock` | Return to typing |

**Emergency shortcuts: `Ctrl+Alt+Esc` releases control and returns to Insert mode; `Ctrl+Alt+Backspace` quits Vimouse.**

### Jump and refine

`f` and `Tab` are independent tools. Use either on its own, or combine them:

1. Press `f` to show a **5×5 letter grid** on the current display. Type a cell's letter to jump to its center and return immediately to Normal mode.
2. Continue moving with `hjkl`, or press `Tab` to open a **4×4 grid** centered around the current pointer position.
3. Choose a letter to enter a **3×3 fine grid**, then use `hjkl` to select a cell. Press `Tab` again to subdivide into the final **2×2 grid**.
4. In the refinement tool, press `Space` to finish, or `Enter` to finish and click. Use `z` or Backspace to go back one level. `Esc` cancels and restores the position from before entering the tool.

Letters mark the center of each cell, exactly where the pointer will land. The 3×3 and 2×2 stages use a selection outline without letters. See [grid controls](docs/guide.md#f-快速选块) (Chinese) for the full rules and examples.

### Save frequently used positions

- `m` followed by `a`–`z`: save the current position as a named mark.
- `'` followed by a mark's letter: jump to that position.
- `M` (Shift+m): show marks across your displays, then type a letter to jump. Press Esc to close.
- `''`: swap with the previous jump position.

Named marks are saved automatically and restored on restart. Each records a display name and a relative position, so it adapts to resolution, scaling, and layout changes. Marks for disconnected displays are retained. See [mark persistence](docs/guide.md#mark-持久保存) (Chinese) for the storage format and behavior.

For menu navigation, commands, window switching, and more shortcuts, see the [complete key reference](docs/guide.md#操作) (Chinese), or press `?` in Normal mode:

![Shortcut reference](docs/help.png)

## Manage the service and update

These commands assume Cargo’s executable directory (normally `~/.cargo/bin`) is on PATH.

```sh
vimouse status
vimouse stop
vimouse start
journalctl --user -u vimouse.service -n 40
```

To update, reinstall through Cargo and restart the service. Run `install-service` once when migrating from an earlier installation, or when changing the executable path:

```sh
cargo install --git https://github.com/Yunus4832/vimouse.git --locked --force
vimouse install-service
vimouse stop
vimouse start
```

Updates preserve your configuration and marks. Run either the user service or a foreground `run` instance, one at a time. See [user service management](docs/guide.md#安装用户服务与自启动) (Chinese) for details.

## Uninstall

If you installed the user service, remove it **before** uninstalling the executable:

```sh
vimouse uninstall-service
cargo uninstall vimouse
```

If you only ran Vimouse in the foreground, exit it and run `cargo uninstall vimouse` directly. For a custom Cargo installation root, pass the same `--root` to `cargo uninstall`.

`uninstall-service` stops and disables the main service and any previous separate HUD service, removes their unit files and managed runtime session settings, and reloads systemd. It can be run more than once and preserves custom drop-ins. Configuration, saved marks, and device permissions configured by `init` are kept. For manually installed binaries, remove the installed executable instead of running `cargo uninstall`.

To also delete your configuration and saved marks, run the following **optional, permanent data removal** after stopping Vimouse:

```sh
rm -rf -- "${XDG_CONFIG_HOME:-$HOME/.config}/vimouse"
rm -rf -- "${XDG_STATE_HOME:-$HOME/.local/state}/vimouse"
```

See [uninstallation details](docs/guide.md#卸载) (Chinese) for retained permission settings.

## Configuration and documentation

For a local checkout, install with `cargo install --path . --locked`. The standalone `vimouse install` command remains available for manually downloaded binaries.

Vimouse works with its defaults. To customize speeds, the mode-switch key, or the idle timeout, copy the example configuration, edit it, and restart:

```sh
mkdir -p ~/.config/vimouse
cp config.example.toml ~/.config/vimouse/config.toml
```

The detailed documentation is currently in Chinese:

- [User and development guide](docs/guide.md): complete shortcuts, permissions, mark format, multiple displays, compatibility, and static builds.
- [Architecture and hardware checks](docs/architecture.md): code structure and validation procedures.
- [Changelog](CHANGELOG.md) · [Shortcut proposals](docs/shortcuts-roadmap.md).

Licensed under [Apache-2.0](LICENSE). Project and third-party notices are available in [NOTICE](NOTICE). You can also run `vimouse licenses` to view the notices embedded in the binary.
