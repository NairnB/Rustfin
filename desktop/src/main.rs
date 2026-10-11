//! Desktop shell: winit window + glutin GL context, the core app drawn with egui, libmpv video underneath.
//!   rustfin [--server URL] [--data-dir DIR] [--size WxH] [--gles2] [--script FILE --shots DIR]
#![cfg_attr(windows, windows_subsystem = "windows")]
mod mpv;
mod player;
mod script;
mod tracks;

use glutin::config::ConfigTemplateBuilder;
use glutin::context::{ContextApi, ContextAttributesBuilder, NotCurrentGlContext, PossiblyCurrentContext, Version};
use glutin::display::{GetGlDisplay, GlDisplay};
use glutin::surface::{GlSurface, Surface, SwapInterval, WindowSurface};
use glutin_winit::{DisplayBuilder, GlWindow};
use jf_core::api::{Client, DeviceInfo, ItemKind};
use jf_core::app::{App, AppOptions};
use jf_core::platform::{Key, PlayRequest, Player, PlayerState};
use jf_core::settings::FileStorage;
use jf_core::ui::player::PlayOptions;
use raw_window_handle::HasWindowHandle;
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key as WKey, NamedKey};
use winit::window::{Window, WindowId};

/// Our builds pre-fill this server on the Connect screen (R-SET-3); empty for the friend's build.
const DEFAULT_SERVER: Option<&str> = option_env!("JF_DEFAULT_SERVER");

struct Args {
    server: Option<String>,
    data_dir: Option<PathBuf>,
    w: u32,
    h: u32,
    gles2: bool,
    script: Option<PathBuf>,
    shots: PathBuf,
    playtest: bool,
    user: Option<String>,
    pass: Option<String>,
    audio: Option<i32>,
    sub: Option<Option<i32>>,
    screen: Option<String>,
}

fn args() -> Args {
    let mut a = Args {
        server: DEFAULT_SERVER.map(String::from),
        data_dir: None,
        w: 1600,
        h: 900,
        gles2: false,
        script: None,
        shots: PathBuf::from("."),
        playtest: false,
        user: None,
        pass: None,
        audio: None,
        sub: None,
        screen: None,
    };
    let v: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < v.len() {
        let next = v.get(i + 1).cloned().unwrap_or_default();
        match v[i].as_str() {
            "--server" => {
                a.server = Some(next);
                i += 1
            }
            "--data-dir" => {
                a.data_dir = Some(next.into());
                i += 1
            }
            "--script" => {
                a.script = Some(next.into());
                i += 1
            }
            "--shots" => {
                a.shots = next.into();
                i += 1
            }
            "--size" => {
                if let Some((w, h)) = next.split_once('x') {
                    a.w = w.parse().unwrap_or(a.w);
                    a.h = h.parse().unwrap_or(a.h);
                }
                i += 1
            }
            "--gles2" => a.gles2 = true,
            "--playtest" => a.playtest = true,
            "--user" => {
                a.user = Some(next);
                i += 1
            }
            "--pass" => {
                a.pass = Some(next);
                i += 1
            }
            "--audio" => {
                a.audio = next.parse().ok();
                i += 1
            }
            "--sub" => {
                let v = if next == "off" { None } else { Some(next.parse().ok()) };
                a.sub = v;
                i += 1
            }
            "--screen" => {
                a.screen = Some(next);
                i += 1
            }
            _ => {}
        }
        i += 1;
    }
    a
}

/// R-PLAT-2: per-OS data dir. A folder from before the rename (JellyfinDesktop) is moved once, keeping the sign-in.
fn default_data_dir() -> PathBuf {
    let (new, old) = data_dirs("Rustfin", "rustfin", "JellyfinDesktop", "jellyfin-desktop");
    if !new.exists() && old.is_dir() {
        let _ = std::fs::rename(&old, &new);
    }
    new
}

fn data_dirs(name: &str, lower: &str, old_name: &str, old_lower: &str) -> (PathBuf, PathBuf) {
    (os_data_dir(name, lower), os_data_dir(old_name, old_lower))
}

fn os_data_dir(name: &str, lower: &str) -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or(home).join(name)
    } else if cfg!(target_os = "macos") {
        home.join("Library/Application Support").join(name)
    } else {
        std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or(home.join(".config")).join(lower)
    }
}

// Fields drop in this order: GL users first, then the context, the surface, and the window last.
struct Gfx {
    egui: egui_glow::EguiGlow,
    gl: Arc<glow::Context>,
    ctx: PossiblyCurrentContext,
    surface: Surface<WindowSurface>,
    window: Window,
}

struct Shell {
    a: Args,
    app: App,
    player: player::MpvPlayer,
    gfx: Option<Gfx>,
    keys: Vec<Key>,
    runner: Option<script::Runner>,
    playtest: Option<Playtest>,
    last: Instant,
    started: Instant,
    first_frame: bool,
    first_poster: bool,
    /// Where scripted mouse commands last put the pointer (points).
    pointer: egui::Pos2,
    /// Tour mode: when the next frame is due (None = at once).
    wake: Option<Instant>,
}

/// Quit teardown: mpv frees its render context while the GL context is still current, then egui's GL objects, and
/// only then do the fields drop (context, surface, window).
impl Drop for Shell {
    fn drop(&mut self) {
        self.player.release();
        if let Some(g) = self.gfx.as_mut() {
            g.egui.destroy();
        }
    }
}

/// `--playtest`: drive the mpv player directly against a throwaway server (packet P1).
struct Playtest {
    rx: mpsc::Receiver<Result<PlayRequest, String>>,
    shots: PathBuf,
    deadline: Instant,
    printed: bool,
}

impl Playtest {
    fn spawn(
        server: String,
        user: String,
        pass: String,
        audio: Option<i32>,
        sub: Option<Option<i32>>,
        shots: PathBuf,
    ) -> Playtest {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let res = playtest_worker(&server, &user, &pass, audio, sub);
            let _ = tx.send(res);
        });
        Playtest { rx, shots, deadline: Instant::now() + Duration::from_secs(30), printed: false }
    }
}

/// Sign in, find "Night Shift Stories" S01E01, fetch PlaybackInfo and build the request.
fn playtest_worker(
    server: &str,
    user: &str,
    pass: &str,
    audio: Option<i32>,
    sub: Option<Option<i32>>,
) -> Result<PlayRequest, String> {
    let device =
        DeviceInfo { name: "playtest".into(), id: "playtest-device".into(), version: env!("CARGO_PKG_VERSION").into() };
    let (mut client, _) = Client::connect(server, device).map_err(|e| e.to_string())?;
    client.sign_in(user, pass).map_err(|e| e.to_string())?;
    let results = client.search("Night Shift Stories").map_err(|e| e.to_string())?;
    let series = results
        .into_iter()
        .find(|i| i.kind == ItemKind::Series && i.name == "Night Shift Stories")
        .ok_or_else(|| "series \"Night Shift Stories\" not found".to_string())?;
    let eps = client.episodes(&series.id, None).map_err(|e| e.to_string())?;
    let ep = eps
        .into_iter()
        .find(|e| e.parent_index_number == Some(1) && e.index_number == Some(1))
        .ok_or_else(|| "S01E01 not found".to_string())?;
    let info = client.playback_info(&ep.id).map_err(|e| e.to_string())?;
    let opts = PlayOptions { from_start: true, audio, subtitle: sub, ..Default::default() };
    let mut req = jf_core::playreq::build(&client, &ep, &info, &opts).ok_or_else(|| "no media source".to_string())?;
    // Resume check: JF_PLAYTEST_START=<secs> starts mid-file like a resume would.
    if let Some(s) = std::env::var("JF_PLAYTEST_START").ok().and_then(|v| v.parse().ok()) {
        req.start_secs = s;
    }
    Ok(req)
}

impl Shell {
    fn create(&mut self, el: &ActiveEventLoop) {
        let attrs = Window::default_attributes()
            .with_title("Rustfin")
            .with_inner_size(winit::dpi::PhysicalSize::new(self.a.w, self.a.h));
        let template = ConfigTemplateBuilder::new().with_depth_size(0).with_stencil_size(0);
        let (window, config) = DisplayBuilder::new()
            .with_window_attributes(Some(attrs))
            .build(el, template, |mut c| c.next().expect("no GL config"))
            .expect("display");
        let window = window.expect("window");
        let display = config.display();
        let raw = window.window_handle().ok().map(|h| h.as_raw());
        let api = if self.a.gles2 { ContextApi::Gles(Some(Version::new(2, 0))) } else { ContextApi::OpenGl(None) };
        let attrs = ContextAttributesBuilder::new().with_context_api(api).build(raw);
        let fallback = ContextAttributesBuilder::new().with_context_api(ContextApi::Gles(None)).build(raw);
        let not_current = unsafe {
            display.create_context(&config, &attrs).or_else(|_| display.create_context(&config, &fallback)).expect("GL")
        };
        let surf_attrs = window.build_surface_attributes(Default::default()).expect("surface attrs");
        let surface = unsafe { display.create_window_surface(&config, &surf_attrs).expect("surface") };
        let ctx = not_current.make_current(&surface).expect("make current");
        let vsync = if self.runner.is_some() { SwapInterval::DontWait } else { SwapInterval::Wait(NonZeroU32::MIN) };
        let _ = surface.set_swap_interval(&ctx, vsync);
        let gl = Arc::new(unsafe { glow::Context::from_loader_function_cstr(|s| display.get_proc_address(s)) });
        let shader = if self.a.gles2 { Some(egui_glow::ShaderVersion::Es100) } else { None };
        let egui = egui_glow::EguiGlow::new(el, gl.clone(), shader, None, true);
        let d2 = display.clone();
        self.player = player::MpvPlayer::new(Box::new(move || {
            let d = d2.clone();
            Box::new(move |n: &std::ffi::CStr| d.get_proc_address(n))
        }));
        if self.a.playtest {
            self.player.set_null_audio(true);
        }
        self.gfx = Some(Gfx { window, surface, ctx, gl, egui });
    }

    fn frame(&mut self, el: &ActiveEventLoop) {
        if self.playtest.is_some() {
            self.playtest_frame();
            return;
        }
        let now = Instant::now();
        let frame_ms = now.duration_since(self.last).as_secs_f32() * 1000.0;
        self.last = now;
        let g = self.gfx.as_mut().unwrap();
        let size = g.window.inner_size();
        let mut step = script::Step::default();
        if let Some(r) = self.runner.as_mut() {
            step = r.step(self.app.screen_name(), self.app.is_idle(), frame_ms);
        }
        if let Some(t) = &step.text {
            g.egui.egui_winit.egui_input_mut().events.push(egui::Event::Text(t.clone()));
        }
        // Scripted mouse: pixels → points (ppp as below), fed to egui like real winit pointer events.
        if let Some(m) = step.mouse {
            let ppp = (size.height as f32 / 1080.0).max(0.75);
            let input = g.egui.egui_winit.egui_input_mut();
            let pos = self.pointer;
            match m {
                script::Mouse::Move(x, y) => {
                    self.pointer = egui::pos2(x / ppp, y / ppp);
                    input.events.push(egui::Event::PointerMoved(self.pointer));
                }
                script::Mouse::Press | script::Mouse::Release => input.events.push(egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: m == script::Mouse::Press,
                    modifiers: Default::default(),
                }),
                script::Mouse::Scroll(dx, dy) => input.events.push(egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(dx / ppp, dy / ppp),
                    modifiers: Default::default(),
                }),
                script::Mouse::Back => self.keys.extend(mouse_key(MouseButton::Back)),
                script::Mouse::Forward => self.keys.extend(mouse_key(MouseButton::Forward)),
            }
        }
        let mut keys = std::mem::take(&mut self.keys);
        // Scripted Backspace while typing edits the text field instead of going Back.
        for &k in &step.keys {
            if k == Key::Backspace {
                g.egui.egui_winit.egui_input_mut().events.push(egui::Event::Key {
                    key: egui::Key::Backspace,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: Default::default(),
                });
            } else {
                keys.push(k);
            }
        }
        // Remote-style keys win over a resting mouse: after a key press egui forgets the pointer until the mouse
        // really moves (winit then sends CursorMoved again), so hover can't keep pulling focus back.
        if !keys.is_empty() {
            g.egui.egui_winit.egui_input_mut().events.push(egui::Event::PointerGone);
        }
        unsafe {
            use glow::HasContext;
            g.gl.clear_color(0.0, 0.0, 0.0, 1.0);
            g.gl.clear(glow::COLOR_BUFFER_BIT);
        }
        self.player.pump();
        if self.app.shows_video() {
            self.player.draw_video(size.width, size.height);
        }
        let ppp = (size.height as f32 / 1080.0).max(0.75); // R-THEME-3
        {
            let (app, player) = (&mut self.app, &mut self.player);
            g.egui.run(&g.window, |ctx| {
                ctx.set_pixels_per_point(ppp);
                app.frame(ctx, player, &keys);
            });
        }
        // G4 / P-PLAY-18: apply the window request the player screen made last frame.
        if let Some(fs) = self.app.take_shell_requests().fullscreen {
            let want = if fs { Some(winit::window::Fullscreen::Borderless(None)) } else { None };
            if g.window.fullscreen() != want {
                g.window.set_fullscreen(want);
            }
        }
        g.egui.paint(&g.window);
        if self.first_frame {
            self.first_frame = false;
            println!("MARK first-frame {}", self.started.elapsed().as_millis());
        }
        if !self.first_poster && self.app.images.loaded_total > 0 {
            self.first_poster = true;
            println!("MARK first-poster {}", self.started.elapsed().as_millis());
        }
        if let Some(name) = &step.shot {
            let path = self.a.shots.join(format!("{name}.png"));
            shoot_png(&g.gl, size.width, size.height, &path);
        }
        g.surface.swap_buffers(&g.ctx).expect("swap");
        // Tour mode draws flat out (no vsync). Draw again at once only when something can change: script input,
        // a step that acts now, an egui animation, video; otherwise poll every 16 ms while loading, else 33 ms.
        if let Some(r) = &self.runner {
            let input = !step.keys.is_empty() || step.text.is_some() || step.mouse.is_some();
            let now_needed = input
                || r.acts_now(self.app.is_idle())
                || self.app.shows_video()
                || g.egui.egui_ctx.has_requested_repaint();
            self.wake = if now_needed {
                None
            } else if !self.app.is_idle() {
                Some(Instant::now() + Duration::from_millis(16))
            } else {
                Some(Instant::now() + Duration::from_millis(33))
            };
        }
        if step.quit {
            self.app.shutdown(&mut self.player);
            self.player.stop();
            el.exit();
        }
    }

    /// `--playtest` frame: the video alone on screen, driven to the 2.5 s mark then reported.
    fn playtest_frame(&mut self) {
        let g = self.gfx.as_mut().unwrap();
        let size = g.window.inner_size();
        unsafe {
            use glow::HasContext;
            g.gl.clear_color(0.0, 0.0, 0.0, 1.0);
            g.gl.clear(glow::COLOR_BUFFER_BIT);
        }
        self.player.pump();
        self.player.draw_video(size.width, size.height);

        let pt = self.playtest.as_mut().unwrap();
        if let Ok(result) = pt.rx.try_recv() {
            match result {
                Ok(req) => self.player.load(&req),
                Err(e) => {
                    eprintln!("PLAYTEST FAILED: {e}");
                    std::process::exit(3);
                }
            }
        }
        if let PlayerState::Failed(msg) = self.player.state() {
            eprintln!("PLAYTEST FAILED: {msg}");
            std::process::exit(3);
        }

        let pos = self.player.position();
        if pos >= 2.5 && !pt.printed {
            pt.printed = true;
            let aid = self.player.get_prop("aid").unwrap_or_default();
            let sid = self.player.get_prop("sid").unwrap_or_default();
            let sub = self.player.get_prop("sub-text").unwrap_or_default();
            let lang = self.player.get_prop("current-tracks/audio/lang").unwrap_or_default();
            println!("PLAYTEST pos={pos:.2} aid={aid} sid={sid} sub=\"{sub}\" audio-lang={lang}");
            let path = pt.shots.join("playtest.png");
            shoot_png(&g.gl, size.width, size.height, &path);
            std::process::exit(0);
        }
        if Instant::now() >= pt.deadline {
            eprintln!("PLAYTEST FAILED: no progress within 30 s (pos {pos:.2})");
            std::process::exit(3);
        }

        g.surface.swap_buffers(&g.ctx).expect("swap");
    }
}

fn shoot_png(gl: &glow::Context, w: u32, h: u32, path: &std::path::Path) {
    use glow::HasContext;
    let mut px = vec![0u8; (w * h * 4) as usize];
    unsafe {
        gl.read_pixels(
            0,
            0,
            w as i32,
            h as i32,
            glow::RGBA,
            glow::UNSIGNED_BYTE,
            glow::PixelPackData::Slice(Some(&mut px)),
        );
    }
    let Some(mut img) = image::RgbaImage::from_raw(w, h, px) else { return };
    image::imageops::flip_vertical_in_place(&mut img);
    for p in img.pixels_mut() {
        p.0[3] = 255;
    }
    match img.save(path) {
        Ok(_) => println!("SHOT {}", path.display()),
        Err(e) => eprintln!("SHOT FAILED {}: {e}", path.display()),
    }
}

/// R-PLAT-3 key map. Backspace and Space only act as Back / Play-pause when no text field has focus.
fn map_key(k: &WKey, typing: bool) -> Option<Key> {
    Some(match k {
        WKey::Named(NamedKey::ArrowUp) => Key::Up,
        WKey::Named(NamedKey::ArrowDown) => Key::Down,
        WKey::Named(NamedKey::ArrowLeft) => Key::Left,
        WKey::Named(NamedKey::ArrowRight) => Key::Right,
        WKey::Named(NamedKey::Enter) => Key::Enter,
        WKey::Named(NamedKey::Escape) | WKey::Named(NamedKey::BrowserBack) => Key::Back,
        WKey::Named(NamedKey::Backspace) if !typing => Key::Back,
        WKey::Named(NamedKey::Space) if !typing => Key::PlayPause,
        WKey::Named(NamedKey::MediaPlayPause) => Key::PlayPause,
        WKey::Named(NamedKey::Delete) if !typing => Key::Delete,
        WKey::Character(c) if !typing => match c.as_str() {
            "i" => Key::Info,
            "f" => Key::Fullscreen,
            "m" => Key::Mute,
            "n" => Key::Next,
            "p" => Key::Previous,
            "c" => Key::Chapters,
            "s" => Key::Stats,
            "q" => Key::Queue,
            "+" | "=" => Key::VolumeUp,
            "-" => Key::VolumeDown,
            _ => return None,
        },
        _ => return None,
    })
}

/// Mouse side buttons: 4 = Back, 5 = Forward (like a browser).
fn mouse_key(b: MouseButton) -> Option<Key> {
    match b {
        MouseButton::Back => Some(Key::Back),
        MouseButton::Forward => Some(Key::Forward),
        _ => None,
    }
}

impl ApplicationHandler for Shell {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.gfx.is_none() {
            self.create(el);
        }
    }
    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, ev: WindowEvent) {
        let Some(g) = self.gfx.as_mut() else { return };
        let typing = g.egui.egui_ctx.wants_keyboard_input();
        let r = g.egui.on_window_event(&g.window, &ev);
        // Not for RedrawRequested itself: egui answers "repaint" to it, and every frame would order the next one.
        if r.repaint && !matches!(ev, WindowEvent::RedrawRequested) {
            g.window.request_redraw();
        }
        match ev {
            WindowEvent::CloseRequested => {
                self.app.shutdown(&mut self.player);
                self.player.stop();
                el.exit()
            }
            WindowEvent::Resized(s) => {
                g.surface.resize(
                    &g.ctx,
                    NonZeroU32::new(s.width.max(1)).unwrap(),
                    NonZeroU32::new(s.height.max(1)).unwrap(),
                );
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                if let Some(k) = map_key(&event.logical_key, typing) {
                    self.keys.push(k);
                }
            }
            WindowEvent::MouseInput { state: ElementState::Pressed, button, .. } => {
                self.keys.extend(mouse_key(button));
            }
            WindowEvent::RedrawRequested => self.frame(el),
            _ => {}
        }
    }
    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        if let Some(g) = &self.gfx {
            match self.wake {
                Some(t) if Instant::now() < t => el.set_control_flow(ControlFlow::WaitUntil(t)),
                _ => {
                    el.set_control_flow(ControlFlow::Poll);
                    g.window.request_redraw();
                }
            }
        }
    }
}

fn main() {
    let started = Instant::now();
    let a = args();
    let dir = a.data_dir.clone().unwrap_or_else(default_data_dir);
    let store = Arc::new(FileStorage::new(dir).expect("data dir"));
    let host =
        std::env::var("COMPUTERNAME").or_else(|_| std::env::var("HOSTNAME")).unwrap_or_else(|_| "Desktop".into());
    let app = App::new(
        store,
        AppOptions { default_server: a.server.clone(), device_name: host, start_screen: a.screen.clone() },
    );
    let runner = a.script.as_ref().map(|p| {
        let src = std::fs::read_to_string(p).expect("script");
        script::Runner::new(script::parse(&src).unwrap_or_else(|e| panic!("{e}")), started)
    });
    let el = EventLoop::new().expect("event loop");
    el.set_control_flow(ControlFlow::Poll);
    let playtest = if a.playtest {
        let server = a.server.clone().expect("--playtest needs --server");
        let user = a.user.clone().expect("--playtest needs --user");
        let pass = a.pass.clone().expect("--playtest needs --pass");
        Some(Playtest::spawn(server, user, pass, a.audio, a.sub, a.shots.clone()))
    } else {
        None
    };
    let mut shell = Shell {
        a,
        app,
        player: player::MpvPlayer::new(Box::new(|| Box::new(|_: &std::ffi::CStr| std::ptr::null()))),
        gfx: None,
        keys: vec![],
        runner,
        playtest,
        last: Instant::now(),
        started,
        first_frame: true,
        first_poster: false,
        pointer: egui::Pos2::ZERO,
        wake: None,
    };
    el.run_app(&mut shell).expect("run");
}
