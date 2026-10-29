//! Native Wayland layer-shell HUD. No Qt, GPU, system fonts, or input bindings.
mod panels;
mod render;
use crate::{
    displays::{Display, Layout},
    engine::Status,
};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, Region},
    delegate_compositor, delegate_layer, delegate_output, delegate_registry, delegate_shm,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    shell::{
        WaylandSurface,
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
    },
    shm::{Shm, ShmHandler, slot::SlotPool},
};
use std::{
    fs,
    os::fd::{AsFd, AsRawFd},
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use wayland_client::{
    Connection, EventQueue, QueueHandle,
    globals::registry_queue_init,
    protocol::{wl_output, wl_shm, wl_surface},
};

struct Surface {
    output: wl_output::WlOutput,
    layer: LayerSurface,
    size: (i32, i32),
    scale: i32,
    configured: bool,
    dirty: bool,
    last: String,
}
struct Hud {
    registry: RegistryState,
    outputs: OutputState,
    compositor: CompositorState,
    layer_shell: LayerShell,
    shm: Shm,
    pool: SlotPool,
    surfaces: Vec<Surface>,
    render: bool,
    status: Option<Status>,
    error: Option<String>,
}

fn connect(render: bool) -> Result<(Connection, EventQueue<Hud>, Hud), String> {
    let conn = Connection::connect_to_env().map_err(|e| {
        format!(
            "Cannot connect to Wayland: {e}. Run as your desktop user inside a Wayland session."
        )
    })?;
    let (globals, mut queue) = registry_queue_init::<Hud>(&conn).map_err(|e| e.to_string())?;
    let qh = queue.handle();
    let compositor = CompositorState::bind(&globals, &qh).map_err(|e| e.to_string())?;
    let layer_shell=LayerShell::bind(&globals,&qh).map_err(|_|"This compositor does not advertise wlr-layer-shell. The native HUD requires layer-shell support; input-only operation is available with --no-hud.".to_string())?;
    let shm = Shm::bind(&globals, &qh).map_err(|e| e.to_string())?;
    let pool = SlotPool::new(4096, &shm).map_err(|e| e.to_string())?;
    let mut hud = Hud {
        registry: RegistryState::new(&globals),
        outputs: OutputState::new(&globals, &qh),
        compositor,
        layer_shell,
        shm,
        pool,
        surfaces: Vec::new(),
        render,
        status: None,
        error: None,
    };
    // Includes xdg-output events emitted in response to the first roundtrip.
    queue.roundtrip(&mut hud).map_err(|e| e.to_string())?;
    queue.roundtrip(&mut hud).map_err(|e| e.to_string())?;
    Ok((conn, queue, hud))
}

/// Read-only protocol check; never creates a surface or grabs any input.
pub fn check() -> Result<(), String> {
    if unsafe { libc::geteuid() } == 0 {
        return Err("The HUD must run as your desktop user. Configure input permissions, then run without sudo; use --no-hud for input-only operation.".into());
    }
    connect(false).map(|_| ())
}

/// Native logical geometry fallback when optional desktop utilities are absent.
pub fn layout() -> Result<Layout, String> {
    let (_, _, hud) = connect(false)?;
    Layout::new(
        hud.outputs
            .outputs()
            .filter_map(|o| hud.display(&o))
            .collect(),
    )
}

pub fn run(directory: &Path, preview: bool) -> Result<(), String> {
    check()?;
    let (conn, mut queue, mut hud) = connect(true)?;
    let quit = Arc::new(AtomicBool::new(false));
    for signal in [
        signal_hook::consts::SIGINT,
        signal_hook::consts::SIGTERM,
        signal_hook::consts::SIGHUP,
    ] {
        signal_hook::flag::register(signal, quit.clone()).map_err(|e| e.to_string())?;
    }
    let mut last = Vec::new();
    if preview {
        let config = crate::config::Config::default();
        let mut engine = crate::engine::Engine::with_layout(
            config,
            Layout::new(
                hud.outputs
                    .outputs()
                    .filter_map(|o| hud.display(&o))
                    .collect(),
            )?,
        );
        engine.key(evdev::KeyCode::KEY_CAPSLOCK, 1);
        hud.status = Some(engine.status());
    }
    hud.draw()?;
    queue.roundtrip(&mut hud).map_err(|e| e.to_string())?;
    println!("Vimouse HUD ready (native Wayland)");
    while !quit.load(Ordering::Relaxed) {
        queue
            .dispatch_pending(&mut hud)
            .map_err(|e| e.to_string())?;
        if !preview {
            let bytes = fs::read(directory.join("status.json")).unwrap_or_default();
            if bytes != last {
                hud.status = serde_json::from_slice(&bytes).ok();
                last = bytes;
            }
        }
        hud.draw()?;
        if let Some(error) = hud.error.take() {
            return Err(error);
        }
        conn.flush().map_err(|e| e.to_string())?;
        if let Some(guard) = queue.prepare_read() {
            let mut fd = libc::pollfd {
                fd: conn.as_fd().as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            // SAFETY: fd refers to the live Wayland connection, one valid pollfd.
            let result = unsafe { libc::poll(&mut fd, 1, 25) };
            if result < 0 {
                let error = std::io::Error::last_os_error();
                if error.kind() != std::io::ErrorKind::Interrupted {
                    return Err(error.to_string());
                }
            } else if result > 0 {
                if let Err(error) = guard.read() {
                    if !matches!(&error, wayland_client::backend::WaylandError::Io(e) if e.kind()==std::io::ErrorKind::WouldBlock)
                    {
                        return Err(error.to_string());
                    }
                }
            }
            // Dropping the guard cancels a read on timeout; never block status updates.
        }
    }
    Ok(())
}

impl Hud {
    fn display(&self, output: &wl_output::WlOutput) -> Option<Display> {
        let info = self.outputs.info(output)?;
        let (x, y) = info.logical_position?;
        let (width, height) = info.logical_size?;
        Some(Display {
            name: info.name?,
            x,
            y,
            width,
            height,
        })
    }
    fn ensure_output(&mut self, qh: &QueueHandle<Self>, output: wl_output::WlOutput) {
        if !self.render {
            return;
        }
        if let Some(s) = self.surfaces.iter_mut().find(|s| s.output == output) {
            s.dirty = true;
            return;
        }
        let surface = self.compositor.create_surface(qh);
        let empty = Region::new(&self.compositor).expect("wl_compositor supports regions");
        surface.set_input_region(Some(empty.wl_region()));
        let layer = self.layer_shell.create_layer_surface(
            qh,
            surface,
            Layer::Overlay,
            Some("vimouse"),
            Some(&output),
        );
        layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
        layer.set_size(0, 0);
        layer.set_exclusive_zone(-1);
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        layer.commit();
        self.surfaces.push(Surface {
            output,
            layer,
            size: (1, 1),
            scale: 1,
            configured: false,
            dirty: true,
            last: String::new(),
        });
    }
    fn draw(&mut self) -> Result<(), String> {
        for s in &mut self.surfaces {
            if !s.configured {
                continue;
            }
            let Some(info) = self.outputs.info(&s.output) else {
                continue;
            };
            let name = info.name.as_deref().unwrap_or("");
            let status = self.status.as_ref().filter(|state| {
                (state.mode == crate::engine::Mode::Marks && !state.display.name.is_empty())
                    || state.display.name == name
                    || (state.display.name.is_empty()
                        && self.outputs.outputs().next().as_ref() == Some(&s.output))
            });
            let key = match status {
                Some(state) if state.mode == crate::engine::Mode::Grid => {
                    serde_json::to_string(&(&state.display, &state.grid))
                }
                Some(state)
                    if matches!(
                        state.mode,
                        crate::engine::Mode::Command
                            | crate::engine::Mode::Help
                            | crate::engine::Mode::Marks
                    ) =>
                {
                    serde_json::to_string(state)
                }
                Some(state) if state.mode != crate::engine::Mode::Insert => {
                    serde_json::to_string(&(state.mode, state.speed, &state.mark_save_error))
                }
                _ => Ok(String::new()),
            }
            .map_err(|e| e.to_string())?;
            let scale = info.scale_factor.max(1);
            if !s.dirty && s.last == key && s.scale == scale {
                continue;
            }
            s.scale = scale;
            let (width, height) = s.size;
            let bw = width.checked_mul(scale).ok_or("HUD width overflow")?;
            let bh = height.checked_mul(scale).ok_or("HUD height overflow")?;
            if bw <= 0
                || bh <= 0
                || bw > 32768
                || bh > 32768
                || i64::from(bw) * i64::from(bh) > 64_000_000
            {
                return Err("HUD output is too large".into());
            }
            let (buffer, bytes) = self
                .pool
                .create_buffer(bw, bh, bw * 4, wl_shm::Format::Argb8888)
                .map_err(|e| e.to_string())?;
            render::render(
                &mut render::Canvas {
                    bytes,
                    width: bw,
                    height: bh,
                    scale,
                },
                status,
                name,
                width,
                height,
            );
            let surface = s.layer.wl_surface();
            surface.set_buffer_scale(scale);
            surface.damage_buffer(0, 0, bw, bh);
            buffer.attach_to(surface).map_err(|e| e.to_string())?;
            s.layer.commit();
            s.dirty = false;
            s.last = key;
        }
        Ok(())
    }
}
impl CompositorHandler for Hud {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        surface: &wl_surface::WlSurface,
        _: i32,
    ) {
        if let Some(s) = self
            .surfaces
            .iter_mut()
            .find(|s| s.layer.wl_surface() == surface)
        {
            s.dirty = true;
        }
    }
    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: wl_output::Transform,
    ) {
    }
    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {}
    fn surface_enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
    fn surface_leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
}
impl OutputHandler for Hud {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.outputs
    }
    fn new_output(&mut self, _: &Connection, qh: &QueueHandle<Self>, output: wl_output::WlOutput) {
        self.ensure_output(qh, output);
    }
    fn update_output(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        output: wl_output::WlOutput,
    ) {
        self.ensure_output(qh, output);
    }
    fn output_destroyed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        output: wl_output::WlOutput,
    ) {
        self.surfaces.retain(|s| s.output != output);
    }
}
impl LayerShellHandler for Hud {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, layer: &LayerSurface) {
        self.surfaces.retain(|s| &s.layer != layer);
    }
    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        layer: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _: u32,
    ) {
        if let Some(s) = self.surfaces.iter_mut().find(|s| &s.layer == layer) {
            s.size = (configure.new_size.0 as i32, configure.new_size.1 as i32);
            s.configured = true;
            s.dirty = true;
        }
    }
}
impl ShmHandler for Hud {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}
impl ProvidesRegistryState for Hud {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry
    }
    registry_handlers![OutputState];
}
delegate_compositor!(Hud);
delegate_output!(Hud);
delegate_layer!(Hud);
delegate_shm!(Hud);
delegate_registry!(Hud);
