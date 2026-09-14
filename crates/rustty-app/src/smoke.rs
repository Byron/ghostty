//! Opt-in native integration check; all shells and saved state are test-owned.
use super::*;
use std::fs;

#[cfg(test)]
#[test]
fn hover_measurement_restarts_for_motion_and_leaving_but_not_duplicate_events() {
    let now = Instant::now();
    let mut smoke = Smoke {
        directory: PathBuf::new(),
        offscreen: false,
        hover: true,
        pointer: None,
        stage: 4,
        deadline: now + Duration::from_secs(45),
        next: now,
        original: None,
        closed: None,
        idle_frames: 0,
        progress_started: now,
        progress_frames: 0,
        progress_seconds: 0.0,
        hidden_title_frames: 0,
        header_frames: 0,
        header_prepares: 0,
        events: BTreeMap::new(),
    };
    let position = Some(Pos2::new(100.0, 100.0));
    smoke.pointer(position, 5);
    assert_eq!(smoke.idle_frames, 5);
    let next = smoke.next;
    smoke.pointer(position, 10);
    assert_eq!((smoke.idle_frames, smoke.next), (5, next));
    smoke.pointer(None, 12);
    assert_eq!(smoke.idle_frames, 12);
    smoke.pointer(position, 15);
    assert_eq!(smoke.idle_frames, 15);
}

pub(super) struct Smoke {
    pub directory: PathBuf,
    pub offscreen: bool,
    hover: bool,
    pointer: Option<Pos2>,
    stage: u8,
    deadline: Instant,
    next: Instant,
    original: Option<Id>,
    closed: Option<(Id, Instant)>,
    idle_frames: u64,
    progress_started: Instant,
    progress_frames: u64,
    progress_seconds: f64,
    hidden_title_frames: u64,
    header_frames: u64,
    header_prepares: u64,
    events: BTreeMap<&'static str, u64>,
}
impl Smoke {
    pub fn from_env(loaded: &mut LoadedConfig) -> Result<Option<Self>> {
        let Some(directory) = std::env::var_os("RUSTTY_SMOKE_DIR") else {
            return Ok(None);
        };
        let directory = PathBuf::from(directory);
        fs::create_dir_all(&directory)?;
        for name in ["window.png", "result.json"] {
            match fs::remove_file(directory.join(name)) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        Self::configure(loaded);
        Ok(Some(Self {
            directory,
            offscreen: std::env::var_os("RUSTTY_SMOKE_OFFSCREEN").is_some(),
            hover: std::env::var_os("RUSTTY_SMOKE_HOVER").is_some(),
            pointer: None,
            stage: 0,
            deadline: Instant::now() + Duration::from_secs(45),
            next: Instant::now(),
            original: None,
            closed: None,
            idle_frames: 0,
            progress_started: Instant::now(),
            progress_frames: 0,
            progress_seconds: 0.0,
            hidden_title_frames: 0,
            header_frames: 0,
            header_prepares: 0,
            events: BTreeMap::new(),
        }))
    }
    pub(super) fn configure(loaded: &mut LoadedConfig) {
        let command = config::Command::Direct(vec!["/bin/sh".into(),"-c".into(),r"printf '\033[2J\033[H\033[1;36mRustty native smoke\033[0m\n\033]7;file://localhost/tmp\007\033]9;4;1;65\007'; exec /bin/sh -i".into()]);
        loaded.config.command = Some(command.clone());
        loaded.config.initial_command = Some(command);
        loaded.config.working_directory = Some(PathBuf::from("/tmp"));
        loaded.config.window_save_state = config::WindowSaveState::Always;
        loaded.config.cursor_style_blink = Some(false);
        loaded.config.progress_style = true;
        loaded.config.undo_timeout = Duration::from_secs(5);
        loaded.config.keybinds.retain(|b| !b.flags.global);
    }
    pub fn record(&mut self, event: &'static str) {
        if self.stage == 4 {
            *self.events.entry(event).or_default() += 1;
        }
    }
    pub fn pointer(&mut self, position: Option<Pos2>, frames: u64) {
        if self.hover && self.pointer != position {
            self.input(frames);
        }
        self.pointer = position;
    }
    pub fn input(&mut self, frames: u64) {
        if self.stage == 4 {
            // A developer can keep using an unlocked Mac during this check.
            // Measure a full quiet interval, rather than calling input redraws idle.
            self.idle_frames = frames;
            self.next = Instant::now() + Duration::from_millis(1500);
        }
    }
    pub fn step(&mut self, app: &mut App, event_loop: &ActiveEventLoop) -> Result<bool> {
        if Instant::now() > self.deadline {
            let windows: Vec<_> = app
                .windows
                .values()
                .map(|host| Platform::window_diagnostics(&host.window))
                .collect();
            let progress: Vec<_> = app
                .panes
                .iter()
                .map(|(&id, pane)| (id, pane.activity.progress()))
                .collect();
            return Err(format!(
                "native smoke timed out at stage {}: {:?}; windows: {:?}; pane progress: {:?}",
                self.stage, app.errors, windows, progress
            )
            .into());
        }
        if Instant::now() < self.next {
            return Ok(false);
        }
        let Some(key) = app
            .windows
            .iter()
            .find(|(_, h)| {
                app.index(h.id)
                    .is_some_and(|i| !app.workspace.windows[i].quick)
            })
            .map(|(key, _)| *key)
        else {
            return Ok(false);
        };
        let mut host = app.windows.remove(&key).unwrap();
        let result = self.step_window(app, event_loop, &mut host);
        app.windows.insert(key, host);
        app.reconcile(event_loop);
        result
    }
    fn step_window(
        &mut self,
        app: &mut App,
        event_loop: &ActiveEventLoop,
        host: &mut Host,
    ) -> Result<bool> {
        let text = |app: &App, id: Id| {
            app.panes
                .get(&id)
                .and_then(|p| p.session.terminal().ok().map(|t| t.plain_text()))
                .unwrap_or_default()
        };
        let pane = app.focused(host.id).ok_or("no active pane")?;
        match self.stage {
            0 => {
                if host.frames == 0 || !text(app, pane).contains("Rustty native smoke") {
                    return Ok(false);
                }
                self.original = Some(pane);
                app.write(pane, b"printf '\\122USTTY_INPUT_OK\\n'\r".to_vec());
                eprintln!("Native smoke: shell output received");
                self.stage = 1;
            }
            1 => {
                if !text(app, pane).contains("RUSTTY_INPUT_OK") {
                    return Ok(false);
                }
                app.action(event_loop, host, Action::NewSplit(Direction::Right), true);
                app.action(event_loop, host, Action::NewSplit(Direction::Down), true);
                app.focus_pane(host.id, self.original.unwrap());
                app.action(event_loop, host, Action::NewSplit(Direction::Down), true);
                if app.tab(host.id).unwrap().panes.len() != 4 {
                    return Err("split creation lost a pane".into());
                }
                app.action(event_loop, host, Action::ToggleQuadrantZoom, true);
                let before = app.focused(host.id).unwrap();
                host.peek = app.tab_mut(host.id).unwrap().begin_peek(config::Modifiers {
                    control: true,
                    super_key: true,
                    ..Default::default()
                });
                if !app.action(
                    event_loop,
                    host,
                    Action::GotoSplit(Direction::QuadrantRight),
                    true,
                ) {
                    return Err("quadrant navigation was blocked".into());
                }
                let peek = host.peek.take().unwrap();
                let target = app.tab_mut(host.id).unwrap().finish_peek(peek);
                if target == before {
                    return Err("quadrant navigation kept old focus".into());
                }
                app.focus_pane(host.id, target);
                app.action(event_loop, host, Action::ToggleQuadrantZoom, true);
                app.action(event_loop, host, Action::NewTab, true);
                if app.panes.len() != 5 {
                    return Err("tab creation did not start a fifth PTY".into());
                }
                app.action(event_loop, host, Action::PreviousTab, true);
                eprintln!("Native smoke: splits, tabs, and quadrant actions passed");
                self.stage = 2;
            }
            2 => {
                if app
                    .panes
                    .keys()
                    .any(|&id| !text(app, id).contains("Rustty native smoke"))
                {
                    return Ok(false);
                }
                if app.panes.values().any(|p| p.cwd != Path::new("/tmp")) {
                    return Err("OSC directory was not decoded before restoration".into());
                }
                if app.panes.values().any(|p| {
                    p.activity.progress()
                        != Some(Progress {
                            state: 1,
                            value: Some(65),
                        })
                }) {
                    // The worker exposes terminal text before the UI drains
                    // its queued progress effects. Wait for both, bounded by
                    // the smoke deadline, rather than racing event delivery.
                    return Ok(false);
                }
                if self.closed.is_none() {
                    self.closed = Some((pane, app.panes[&pane].started));
                    app.action(event_loop, host, Action::CloseSurface, true);
                    self.stage = 5;
                    return Ok(false);
                }
                app.save();
                let restored =
                    Workspace::load(&app.state_path)?.ok_or("workspace was not saved")?;
                if restored.windows[0].tabs.len() != 2
                    || restored.windows[0].tabs[0].panes.len() != 4
                {
                    return Err("restoration lost tabs or splits".into());
                }
                eprintln!(
                    "Native smoke: requesting frame, visible={}, occluded={}, frames={}, format={:?}",
                    host.visible,
                    host.occluded,
                    host.frames,
                    app.painter.render_state().map(|s| s.target_format)
                );
                host.capture = true;
                host.repaint();
                self.stage = 3;
            }
            3 => {
                let capture_path = self.directory.join("window.png");
                if !capture_path.is_file() {
                    if let Some(state) = app.painter.render_state() {
                        state.device.poll(wgpu::PollType::Poll)?;
                    }
                    let mut events = Vec::new();
                    app.painter.handle_screenshots(&mut events);
                    if events.is_empty() {
                        host.capture = true;
                        host.repaint();
                        self.next = Instant::now() + Duration::from_millis(100);
                    }
                    for event in events {
                        if let egui::Event::Screenshot { image, .. } = event {
                            let file = fs::File::create(&capture_path)?;
                            let mut encoder = png::Encoder::new(
                                file,
                                image.width() as u32,
                                image.height() as u32,
                            );
                            encoder.set_color(png::ColorType::Rgba);
                            encoder.set_depth(png::BitDepth::Eight);
                            encoder.write_header()?.write_image_data(
                                &image
                                    .pixels
                                    .iter()
                                    .flat_map(|c| c.to_array())
                                    .collect::<Vec<_>>(),
                            )?;
                        }
                    }
                }
                if capture_path.is_file() {
                    check_pointer_targets(app, host)?;
                    self.idle_frames = host.frames;
                    self.progress_started = Instant::now();
                    app.panes
                        .get_mut(&pane)
                        .unwrap()
                        .activity
                        .progress_reported(3, None, self.progress_started);
                    host.repaint();
                    self.next = self.progress_started + Duration::from_secs(2);
                    self.stage = 7;
                }
            }
            4 => {
                if self.hover
                    && !self.pointer.is_some_and(|position| {
                        host.rects.values().any(|rect| rect.contains(position))
                    })
                {
                    self.input(host.frames);
                    return Ok(false);
                }
                if !app.errors.is_empty() {
                    return Err(format!("native app errors: {:?}", app.errors).into());
                }
                if host.frames.saturating_sub(self.idle_frames) > 4 {
                    return Err(format!(
                        "idle window kept repainting: {} frames; events: {:?}",
                        host.frames - self.idle_frames,
                        self.events
                    )
                    .into());
                }
                let refresh_hz = host
                    .window
                    .current_monitor()
                    .and_then(|monitor| monitor.refresh_rate_millihertz())
                    .map(|rate| f64::from(rate) / 1000.0);
                let report = serde_json::json!({"passed":true,"capture_mode":if self.offscreen { "offscreen" } else { "surface" },"checks":["native-window","metal-wgpu-frame","pty-input-output","four-splits","tab-creation","quadrant-focus-and-zoom","cwd-uri-decoding","osc-progress","progress-animation","hover-scrolling","file-drop-targeting","hidden-tab-titles","retained-pane-content","workspace-roundtrip","undo-keeps-pty","idle-rendering"],"frames":host.frames,"idle_frames":host.frames-self.idle_frames,"hidden_title_frames":self.hidden_title_frames,"header_updates":{"frames":self.header_frames,"pane_prepares":self.header_prepares},"progress_animation":{"frames":self.progress_frames,"seconds":self.progress_seconds,"fps":self.progress_frames as f64/self.progress_seconds,"monitor_refresh_hz":refresh_hz},"panes":app.panes.len(),"idle_phase_events":self.events,"hover_required":self.hover,"pointer":self.pointer.map(|position|[position.x,position.y])});
                fs::write(
                    self.directory.join("result.json"),
                    serde_json::to_vec_pretty(&report)?,
                )?;
                println!("Native smoke passed: {}", self.directory.display());
                return Ok(true);
            }
            5 => {
                let (closed, started) = self.closed.unwrap();
                if app.tab(host.id).unwrap().panes.contains_key(&closed)
                    || !app
                        .panes
                        .get(&closed)
                        .is_some_and(|pane| pane.started == started)
                {
                    return Err("closed terminal was not retained for undo".into());
                }
                if !app.action(event_loop, host, Action::Undo, true) {
                    return Err("close could not be undone".into());
                }
                self.stage = 6;
            }
            6 => {
                let (closed, started) = self.closed.unwrap();
                if !app.tab(host.id).unwrap().panes.contains_key(&closed)
                    || !app
                        .panes
                        .get(&closed)
                        .is_some_and(|pane| pane.started == started)
                    || !text(app, closed).contains("Rustty native smoke")
                {
                    return Err("undo did not restore the same terminal process".into());
                }
                self.stage = 2;
            }
            7 => {
                self.progress_frames = host.frames.saturating_sub(self.idle_frames);
                self.progress_seconds = self.progress_started.elapsed().as_secs_f64();
                if !self.offscreen && self.progress_frames == 0 {
                    return Err("indefinite progress did not render any animation frames".into());
                }
                eprintln!(
                    "Native smoke: indefinite progress rendered {} frames in {:.3}s ({:.1} FPS)",
                    self.progress_frames,
                    self.progress_seconds,
                    self.progress_frames as f64 / self.progress_seconds
                );
                app.panes
                    .get_mut(&pane)
                    .unwrap()
                    .activity
                    .progress_reported(1, Some(65), Instant::now());
                let index = app.index(host.id).unwrap();
                let window = &mut app.workspace.windows[index];
                let hidden = window
                    .tabs
                    .iter_mut()
                    .enumerate()
                    .find(|(index, _)| *index != window.active_tab)
                    .map(|(_, tab)| tab)
                    .ok_or("no hidden tab for title checks")?;
                hidden.title = Some("Background agent".into());
                let id = hidden.focused;
                app.write(id, b"i=0; while [ \"$i\" -lt 20 ]; do printf '\\033]2;hidden-agent-%s\\007' \"$i\"; i=$((i+1)); sleep 0.1; done\r".to_vec());
                host.repaint();
                self.idle_frames = host.frames;
                self.next = Instant::now() + Duration::from_secs(3);
                self.stage = 8;
            }
            8 => {
                let window = &app.workspace.windows[app.index(host.id).unwrap()];
                let hidden = window
                    .tabs
                    .iter()
                    .find(|tab| tab.title.as_deref() == Some("Background agent"))
                    .unwrap();
                if app.panes[&hidden.focused].title != "hidden-agent-19" {
                    return Err("hidden-tab title stream did not finish".into());
                }
                self.hidden_title_frames = host.frames.saturating_sub(self.idle_frames);
                if self.hidden_title_frames > 4 {
                    return Err(format!(
                        "masked hidden-tab titles caused {} redraws",
                        self.hidden_title_frames
                    )
                    .into());
                }
                eprintln!(
                    "Native smoke: 20 masked hidden-tab titles caused {} settling frames",
                    self.hidden_title_frames
                );
                let index = app.index(host.id).unwrap();
                let window = &mut app.workspace.windows[index];
                let hidden = window
                    .tabs
                    .iter_mut()
                    .find(|tab| tab.title.as_deref() == Some("Background agent"))
                    .unwrap();
                hidden.title = None;
                let id = hidden.focused;
                self.header_frames = host.frames;
                self.header_prepares = host.pane_prepares;
                app.write(id, b"i=0; while [ \"$i\" -lt 20 ]; do printf '\\033]2;visible-agent-%s\\007' \"$i\"; i=$((i+1)); sleep 0.1; done\r".to_vec());
                host.repaint();
                self.next = Instant::now() + Duration::from_secs(3);
                self.stage = 9;
            }
            9 => {
                self.header_frames = host.frames.saturating_sub(self.header_frames);
                self.header_prepares = host.pane_prepares.saturating_sub(self.header_prepares);
                let window = &app.workspace.windows[app.index(host.id).unwrap()];
                let hidden = window
                    .tabs
                    .iter()
                    .enumerate()
                    .find(|(index, _)| *index != window.active_tab)
                    .unwrap()
                    .1;
                if app.panes[&hidden.focused].title != "visible-agent-19" || self.header_frames < 10
                {
                    return Err("visible tab-label updates stopped repainting".into());
                }
                // The focused pane can rebuild as its blink phase changes; the
                // other panes must retain their content throughout title updates.
                if self.header_prepares > 8 {
                    return Err(format!(
                        "{} tab-header frames rebuilt panes {} times",
                        self.header_frames, self.header_prepares
                    )
                    .into());
                }
                eprintln!(
                    "Native smoke: {} tab-header frames needed only {} pane preparations",
                    self.header_frames, self.header_prepares
                );
                host.repaint();
                self.idle_frames = host.frames;
                self.next = Instant::now() + Duration::from_millis(1500);
                self.stage = 4;
            }
            _ => unreachable!(),
        }
        Ok(false)
    }
}

fn check_pointer_targets(app: &mut App, host: &mut Host) -> Result<()> {
    let focused = app
        .focused(host.id)
        .ok_or("no focused pane for scrolling")?;
    let hovered = *host
        .rects
        .keys()
        .find(|&&id| id != focused)
        .ok_or("no unfocused pane for scrolling")?;
    let mouse = host.mouse;
    let modifiers = host.modifiers;
    let pointer_in_window = host.egui.is_pointer_in_window();
    let mut saved = Vec::new();
    for id in [focused, hovered] {
        let pane = app.panes.get_mut(&id).unwrap();
        let mut terminal = pane.session.terminal()?;
        let mut fixture = vt::Terminal::new(terminal.cols, terminal.rows, 64);
        fixture.feed("\r\n".repeat(usize::from(terminal.rows) + 12).as_bytes());
        saved.push((
            id,
            std::mem::replace(&mut *terminal, fixture),
            std::mem::take(&mut pane.input),
            std::mem::take(&mut pane.input_bytes),
            pane.mouse_cell.take(),
        ));
        // Hold encoded reports in the existing queue instead of sending them to a shell.
        pane.input.push_back(Vec::new());
    }
    let result = (|| -> Result<()> {
        host.mouse = host.rects[&hovered].center();
        let _ = host.egui.on_window_event(
            &host.window,
            &WindowEvent::CursorMoved {
                device_id: winit::event::DeviceId::dummy(),
                position: LogicalPosition::new(host.mouse.x, host.mouse.y)
                    .to_physical(host.window.scale_factor()),
            },
        );
        for (focused_mode, hovered_mode, shift) in [
            (0, 0, false),
            (1000, 0, false),
            (0, 1000, false),
            (1000, 1000, false),
            (0, 1000, true),
        ] {
            host.modifiers = if shift {
                winit::keyboard::ModifiersState::SHIFT
            } else {
                winit::keyboard::ModifiersState::empty()
            }
            .into();
            for (id, mode) in [(focused, focused_mode), (hovered, hovered_mode)] {
                let mut terminal = app.panes[&id].session.terminal()?;
                terminal.mouse_mode = mode;
                terminal.mouse_format = 1006;
            }
            let reporting = hovered_mode != 0 && !shift;
            let pixels = f64::from(host.fonts.metrics().cell_height);
            for (delta, offset, code) in [
                (MouseScrollDelta::LineDelta(0.0, 1.0), 3, 64),
                (MouseScrollDelta::PixelDelta((0.0, pixels).into()), 6, 64),
                (MouseScrollDelta::LineDelta(0.0, -1.0), 3, 65),
                (MouseScrollDelta::PixelDelta((0.0, -pixels).into()), 0, 65),
            ] {
                app.scroll(host, delta);
                let focused_pane = &app.panes[&focused];
                if app.focused(host.id) != Some(focused)
                    || focused_pane.session.terminal()?.screen().viewport_offset != 0
                    || focused_pane.input.len() != 1
                {
                    return Err(
                        "scrolling affected the focused pane instead of the hovered pane".into(),
                    );
                }
                let pane = app.panes.get_mut(&hovered).unwrap();
                if pane.session.terminal()?.screen().viewport_offset
                    != if reporting { 0 } else { offset }
                    || pane.input.len() != if reporting { 2 } else { 1 }
                    || reporting
                        && !pane
                            .input
                            .back()
                            .unwrap()
                            .starts_with(format!("\x1b[<{code};").as_bytes())
                {
                    return Err(format!("hovered pane did not scroll correctly: {delta:?}, mouse modes {focused_mode}/{hovered_mode}, shift={shift}").into());
                }
                if reporting {
                    pane.input.pop_back();
                    pane.input_bytes = 0;
                }
            }
        }
        // A stale last position after leaving the window must not keep a target.
        let _ = host.egui.on_window_event(
            &host.window,
            &WindowEvent::CursorLeft {
                device_id: winit::event::DeviceId::dummy(),
            },
        );
        app.scroll(host, MouseScrollDelta::LineDelta(0.0, 1.0));
        host.mouse = Pos2::new(-1.0, -1.0);
        let _ = host.egui.on_window_event(
            &host.window,
            &WindowEvent::CursorMoved {
                device_id: winit::event::DeviceId::dummy(),
                position: (0.0, 0.0).into(),
            },
        );
        app.scroll(host, MouseScrollDelta::LineDelta(0.0, 1.0));
        for id in [focused, hovered] {
            let pane = &app.panes[&id];
            if pane.session.terminal()?.screen().viewport_offset != 0 || pane.input.len() != 1 {
                return Err("scrolling outside panes kept a terminal target".into());
            }
        }

        // A Finder drag can enter without updating the ordinary pointer state.
        let _ = Platform::cursor_position(&host.window)?;
        let _ = host.egui.on_window_event(
            &host.window,
            &WindowEvent::CursorLeft {
                device_id: winit::event::DeviceId::dummy(),
            },
        );
        let position = host.rects[&hovered].center();
        let path = Path::new("/tmp/rustty's dropped file.txt");
        for bracketed in [false, true] {
            for (id, enabled) in [(focused, !bracketed), (hovered, bracketed)] {
                app.panes[&id].session.terminal()?.feed(if enabled {
                    b"\x1b[?2004h"
                } else {
                    b"\x1b[?2004l"
                });
            }
            app.drop_file(host, position, path);
            app.drop_file(host, Pos2::ZERO, path);
            let expected: &[u8] = if bracketed {
                b"\x1b[200~'/tmp/rustty'\\''s dropped file.txt' \x1b[201~"
            } else {
                b"'/tmp/rustty'\\''s dropped file.txt' "
            };
            if app.focused(host.id) != Some(focused) || app.panes[&focused].input.len() != 1 {
                return Err("file drop affected the focused pane".into());
            }
            let pane = app.panes.get_mut(&hovered).unwrap();
            if pane.input.len() != 2 || pane.input.back().unwrap() != expected {
                return Err(
                    "file drop did not use the target pane's paste mode and quoted path".into(),
                );
            }
            pane.input.pop_back();
            pane.input_bytes = 0;
        }
        Ok(())
    })();
    for (id, terminal, input, input_bytes, mouse_cell) in saved {
        let pane = app.panes.get_mut(&id).unwrap();
        *pane.session.terminal()? = terminal;
        pane.input = input;
        pane.input_bytes = input_bytes;
        pane.mouse_cell = mouse_cell;
    }
    host.mouse = mouse;
    host.modifiers = modifiers;
    let event = if pointer_in_window {
        WindowEvent::CursorMoved {
            device_id: winit::event::DeviceId::dummy(),
            position: LogicalPosition::new(mouse.x, mouse.y)
                .to_physical(host.window.scale_factor()),
        }
    } else {
        WindowEvent::CursorLeft {
            device_id: winit::event::DeviceId::dummy(),
        }
    };
    let _ = host.egui.on_window_event(&host.window, &event);
    result?;
    eprintln!("Native smoke: scrolling and file drops followed the pointer without changing focus");
    Ok(())
}

/// Render the same host primitives to a texture when no drawable is available
/// (e.g. a locked CI Mac). This does not claim visible surface presentation.
pub(super) fn capture(
    state: &egui_wgpu::RenderState,
    primitives: &[egui::ClippedPrimitive],
    delta: &egui::TexturesDelta,
    size: [u32; 2],
    scale: f32,
    path: &Path,
) -> Result<()> {
    let device = &state.device;
    let queue = &state.queue;
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Rustty host capture"),
        size: wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: state.target_format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let stride = (size[0] * 4).div_ceil(256) * 256;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Rustty host readback"),
        size: u64::from(stride) * u64::from(size[1]),
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    let descriptor = egui_wgpu::ScreenDescriptor {
        size_in_pixels: size,
        pixels_per_point: scale,
    };
    let mut renderer = state.renderer.write();
    for (id, deltas) in &delta.set {
        for delta in deltas {
            renderer.update_texture(device, queue, *id, delta);
        }
    }
    let commands = renderer.update_buffers(device, queue, &mut encoder, primitives, &descriptor);
    {
        let view = texture.create_view(&Default::default());
        let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Rustty host capture"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        renderer.render(&mut pass.forget_lifetime(), primitives, &descriptor);
    }
    drop(renderer);
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(stride),
                rows_per_image: None,
            },
        },
        texture.size(),
    );
    queue.submit(commands.into_iter().chain([encoder.finish()]));
    let (tx, rx) = std::sync::mpsc::channel();
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
    device.poll(wgpu::PollType::wait_indefinitely())?;
    rx.recv_timeout(Duration::from_secs(5))??;
    let mapped = buffer.slice(..).get_mapped_range()?;
    let mut pixels = Vec::with_capacity((size[0] * size[1] * 4) as usize);
    for row in mapped.chunks_exact(stride as usize) {
        for pixel in row[..size[0] as usize * 4].as_chunks::<4>().0 {
            if state.target_format == wgpu::TextureFormat::Bgra8Unorm {
                pixels.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]);
            } else {
                pixels.extend_from_slice(pixel);
            }
        }
    }
    drop(mapped);
    buffer.unmap();
    let mut encoder = png::Encoder::new(fs::File::create(path)?, size[0], size[1]);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(&pixels)?;
    Ok(())
}
