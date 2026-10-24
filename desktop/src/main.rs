//! Desktop shell: winit window + glutin GL context, the core app drawn with egui, libmpv video underneath.
//!   rustfin [--server URL] [--data-dir DIR] [--size WxH] [--gles2] [--tv] [--script FILE --shots DIR | --drive DIR]
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
    /// `--drive DIR`: take commands one at a time from `DIR/in`, answer in `DIR/out` + `DIR/shots` (tools/ux/rf-drive).
    drive: Option<PathBuf>,
    playtest: bool,
    user: Option<String>,
    pass: Option<String>,
    audio: Option<i32>,
    sub: Option<Option<i32>>,
    screen: Option<String>,
    /// `--tv`: run with the TV's remote-first navigation (no sidebar/toolbar shell; the top pill bar on Home).
    tv: bool,
    /// `--poster-candidates <json>`: test-only — replace the poster picker's RemoteImages reply with this fixture.
    poster_candidates: Option<String>,
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
        drive: None,
        playtest: false,
        user: None,
        pass: None,
        audio: None,
        sub: None,
        screen: None,
        tv: false,
        poster_candidates: None,
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
            "--drive" => {
                a.drive = Some(next.into());
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
            "--tv" => a.tv = true,
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
            "--poster-candidates" => {
                a.poster_candidates = Some(next);
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
    /// `record` script command: frames go to this folder, numbered.
    recording: Option<(PathBuf, u32)>,
    playtest: Option<Playtest>,
    last: Instant,
    started: Instant,
    first_frame: bool,
    first_poster: bool,
    /// Where scripted mouse commands last put the pointer (points).
    pointer: egui::Pos2,
    /// Held modifier keys (for the Back / Forward / Search shortcuts).
    mods: winit::keyboard::ModifiersState,
    /// A key repeat (key held down) arrived since the last frame.
    held: bool,
    /// Remote keys physically held down now (pressed but not yet released), for the core's key-down state.
    held_keys: std::collections::HashSet<Key>,
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
            step = r.step(self.app.screen_name(), self.app.is_idle(), frame_ms, self.app.now());
        }
        if let Some(rec) = step.record.take() {
            self.app.set_fixed_step(rec.as_ref().map(|(_, fps)| 1.0 / (*fps).max(1) as f64));
            self.recording = rec.map(|(name, _)| (self.a.shots.join(name), 0));
            if let Some((dir, _)) = &self.recording {
                let _ = std::fs::create_dir_all(dir);
            }
        }
        if let Some(secs) = step.fast_forward {
            self.app.advance_clock(secs);
        }
        if let Some(t) = &step.text {
            g.egui.egui_winit.egui_input_mut().events.push(egui::Event::Text(t.clone()));
        }
        // Scripted mouse: pixels → points (ppp as below), fed to egui like real winit pointer events.
        if let Some(m) = step.mouse {
            // Drive testers see 1280-px-wide shots: their coordinates are in that picture.
            let k = if self.a.drive.is_some() && size.width > 1280 { size.width as f32 / 1280.0 } else { 1.0 };
            let ppp = (size.height as f32 / 1080.0).max(0.75) / k;
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
                script::Mouse::RightPress | script::Mouse::RightRelease => {
                    input.events.push(egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Secondary,
                        pressed: m == script::Mouse::RightPress,
                        modifiers: Default::default(),
                    })
                }
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
        self.app.keys_held = std::mem::take(&mut self.held) || step.held;
        // R-PLAT-3 key-down state: physical keys still held (winit) plus a scripted `hold` command's key.
        let mut down = self.held_keys.clone();
        down.extend(step.down.iter().copied());
        self.app.keys_down = down;
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
        let requests = self.app.take_shell_requests();
        // RR8 / P-USER-1: the native file dialog (modal, on this thread). Scripted runs have no one to answer it.
        if requests.pick_file && self.runner.is_none() {
            let picked =
                rfd::FileDialog::new().add_filter("Pictures", &["png", "jpg", "jpeg", "gif", "webp"]).pick_file();
            if let Some(path) = picked {
                self.app.file_picked(path.to_string_lossy().into_owned());
            }
        }
        if let Some(fs) = requests.fullscreen {
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
        if let (Some((n, at, err)), Some(dir)) = (step.drive_shot.take(), &self.a.drive) {
            // Desk shots show where the pointer is; the TV has none.
            let cursor = (!self.a.tv && self.pointer != egui::Pos2::ZERO).then(|| self.pointer * ppp);
            let msg = err.map_or_else(
                || format!("ok {} {}", self.app.screen_name(), at.elapsed().as_millis()),
                |e| format!("error {}", e.replace('\n', " ")),
            );
            drive_shot(&g.gl, size.width, size.height, dir, n, &msg, cursor);
        }
        if let Some((dir, n)) = self.recording.as_mut() {
            *n += 1;
            shoot_png_quiet(&g.gl, size.width, size.height, &dir.join(format!("{n:05}.png")));
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
    let Some(img) = capture(gl, w, h) else { return };
    match img.save(path) {
        Ok(_) => println!("SHOT {}", path.display()),
        Err(e) => eprintln!("SHOT FAILED {}: {e}", path.display()),
    }
}

/// rf-drive step `n`: `<dir>/shots/NNN.jpg` (1280 px wide, quality 85, the pointer drawn in), then the line
/// `NNN <msg>` appended to `<dir>/out` — the wrapper waits for that line, so the picture is finished before it.
fn drive_shot(
    gl: &glow::Context,
    w: u32,
    h: u32,
    dir: &std::path::Path,
    n: u32,
    msg: &str,
    cursor: Option<egui::Pos2>,
) {
    use std::io::Write;
    if let Some(mut img) = capture(gl, w, h) {
        let scale = if w > 1280 { 1280.0 / w as f32 } else { 1.0 };
        if scale < 1.0 {
            img = image::imageops::resize(&img, 1280, (h as f32 * scale).round() as u32, image::imageops::Lanczos3);
        }
        if let Some(c) = cursor {
            draw_cursor(&mut img, c.x * scale, c.y * scale);
        }
        let rgb = image::DynamicImage::ImageRgba8(img).to_rgb8();
        let path = dir.join("shots").join(format!("{n:03}.jpg"));
        let saved = std::fs::File::create(&path).map_err(|e| e.to_string()).and_then(|f| {
            let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(std::io::BufWriter::new(f), 85);
            rgb.write_with_encoder(enc).map_err(|e| e.to_string())
        });
        if let Err(e) = saved {
            eprintln!("DRIVE SHOT FAILED {}: {e}", path.display());
        }
    }
    let out = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("out"));
    if let Ok(mut f) = out {
        let _ = writeln!(f, "{n:03} {msg}");
    }
}

/// A small white arrow with a dark outline, tip at (x, y) in picture pixels.
fn draw_cursor(img: &mut image::RgbaImage, x: f32, y: f32) {
    const ARROW: [(f32, f32); 7] = [(0., 0.), (0., 16.), (4., 12.), (7., 19.), (9., 18.), (6., 11.), (11., 11.)];
    let k = 1.4;
    let poly: Vec<(f32, f32)> = ARROW.iter().map(|&(px, py)| (x + px * k, y + py * k)).collect();
    let inside = |p: (f32, f32)| {
        let mut c = false;
        for i in 0..poly.len() {
            let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
            if (a.1 > p.1) != (b.1 > p.1) && p.0 < (b.0 - a.0) * (p.1 - a.1) / (b.1 - a.1) + a.0 {
                c = !c;
            }
        }
        c
    };
    let edge = |p: (f32, f32)| {
        (0..poly.len())
            .map(|i| {
                let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
                let (dx, dy) = (b.0 - a.0, b.1 - a.1);
                let t = (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
                ((p.0 - a.0 - t * dx).powi(2) + (p.1 - a.1 - t * dy).powi(2)).sqrt()
            })
            .fold(f32::MAX, f32::min)
    };
    let (x0, y0) = (x.floor() as i32 - 3, y.floor() as i32 - 3);
    for py in y0..y0 + 36 {
        for px in x0..x0 + 24 {
            if px < 0 || py < 0 || px >= img.width() as i32 || py >= img.height() as i32 {
                continue;
            }
            let p = (px as f32 + 0.5, py as f32 + 0.5);
            let c = if inside(p) && edge(p) > 1.0 {
                [255, 255, 255, 255]
            } else if inside(p) || edge(p) <= 1.8 {
                [20, 20, 20, 255]
            } else {
                continue;
            };
            img.put_pixel(px as u32, py as u32, image::Rgba(c));
        }
    }
}

/// A `record` frame: like `shoot_png`, without a line per frame.
fn shoot_png_quiet(gl: &glow::Context, w: u32, h: u32, path: &std::path::Path) {
    if let Some(Err(e)) = capture(gl, w, h).map(|img| img.save(path)) {
        eprintln!("RECORD FAILED {}: {e}", path.display());
    }
}

fn capture(gl: &glow::Context, w: u32, h: u32) -> Option<image::RgbaImage> {
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
    let mut img = image::RgbaImage::from_raw(w, h, px)?;
    image::imageops::flip_vertical_in_place(&mut img);
    for p in img.pixels_mut() {
        p.0[3] = 255;
    }
    Some(img)
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

/// NAV shortcuts: Alt+Left / Alt+Right (Windows, Linux), Cmd+[ / Cmd+] (Mac) → Back / Forward; Cmd/Ctrl+F → Search.
/// They work while typing too (they are not text).
fn nav_shortcut(k: &WKey, m: winit::keyboard::ModifiersState) -> Option<Key> {
    let cmd = m.super_key() || m.control_key();
    match k {
        WKey::Named(NamedKey::ArrowLeft) if m.alt_key() => Some(Key::Back),
        WKey::Named(NamedKey::ArrowRight) if m.alt_key() => Some(Key::Forward),
        WKey::Named(NamedKey::BrowserForward) => Some(Key::Forward),
        WKey::Named(NamedKey::BrowserSearch) => Some(Key::Search),
        WKey::Character(c) if cmd => match c.to_lowercase().as_str() {
            "[" => Some(Key::Back),
            "]" => Some(Key::Forward),
            "f" => Some(Key::Search),
            _ => None,
        },
        _ => None,
    }
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
            WindowEvent::KeyboardInput { event, .. } => {
                let remote = if event.state == ElementState::Pressed {
                    self.held |= event.repeat;
                    let k = nav_shortcut(&event.logical_key, self.mods).or_else(|| {
                        if self.mods.super_key() || self.mods.control_key() || self.mods.alt_key() {
                            None
                        } else {
                            map_key(&event.logical_key, typing)
                        }
                    });
                    if let Some(k) = k {
                        self.held_keys.insert(k);
                        self.keys.push(k);
                    }
                    return;
                } else {
                    // Released: forget the key so the app's key-down state (TV hold-OK) goes false.
                    map_key(&event.logical_key, typing).or_else(|| nav_shortcut(&event.logical_key, self.mods))
                };
                if let Some(k) = remote {
                    self.held_keys.remove(&k);
                }
            }
            WindowEvent::ModifiersChanged(m) => self.mods = m.state(),
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
    // `--poster-candidates @path` reads the fixture from a file (the tours); otherwise the value is the JSON itself.
    // Relative `file://` image URLs in a fixture file resolve against that file's folder, so the tour works from any cwd.
    let mut poster_fixture_dir = None;
    let poster_candidates = a.poster_candidates.clone().map(|v| match v.strip_prefix('@') {
        Some(path) => {
            let json = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("--poster-candidates {path}: {e}"));
            let dir = std::fs::canonicalize(path)
                .ok()
                .and_then(|p| p.parent().map(|d| d.to_path_buf()))
                .unwrap_or_else(|| std::path::PathBuf::from("."));
            let out = jf_core::api::poster::absolutize_fixture(&json, &dir);
            poster_fixture_dir = Some(dir);
            out
        }
        None => v,
    });
    let app = App::new(
        store,
        AppOptions {
            default_server: a.server.clone(),
            device_name: host,
            start_screen: a.screen.clone(),
            desktop: !a.tv,
            poster_candidates,
            poster_fixture_dir,
        },
    );
    let runner = match (&a.drive, &a.script) {
        (Some(dir), _) => Some(script::Runner::drive(dir, a.tv, started)),
        (None, p) => p.as_ref().map(|p| {
            let src = std::fs::read_to_string(p).expect("script");
            script::Runner::new(script::parse(&src).unwrap_or_else(|e| panic!("{e}")), started)
        }),
    };
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
        recording: None,
        playtest,
        last: Instant::now(),
        started,
        first_frame: true,
        first_poster: false,
        pointer: egui::Pos2::ZERO,
        mods: Default::default(),
        held: false,
        held_keys: std::collections::HashSet::new(),
        wake: None,
    };
    el.run_app(&mut shell).expect("run");
}

#[cfg(test)]
mod tests {
    use super::*;
    use winit::keyboard::ModifiersState as M;

    #[test]
    fn back_forward_search_shortcuts() {
        let ch = |s: &str| WKey::Character(s.into());
        assert_eq!(nav_shortcut(&WKey::Named(NamedKey::ArrowLeft), M::ALT), Some(Key::Back));
        assert_eq!(nav_shortcut(&WKey::Named(NamedKey::ArrowRight), M::ALT), Some(Key::Forward));
        assert_eq!(nav_shortcut(&WKey::Named(NamedKey::ArrowLeft), M::empty()), None);
        assert_eq!(nav_shortcut(&ch("["), M::SUPER), Some(Key::Back));
        assert_eq!(nav_shortcut(&ch("]"), M::SUPER), Some(Key::Forward));
        assert_eq!(nav_shortcut(&ch("f"), M::CONTROL), Some(Key::Search));
        assert_eq!(nav_shortcut(&ch("F"), M::SUPER), Some(Key::Search));
        assert_eq!(nav_shortcut(&ch("f"), M::empty()), None);
        assert_eq!(mouse_key(MouseButton::Back), Some(Key::Back));
        assert_eq!(mouse_key(MouseButton::Forward), Some(Key::Forward));
        assert_eq!(mouse_key(MouseButton::Left), None);
        // Esc is Back even while typing; Backspace only when not typing.
        assert_eq!(map_key(&WKey::Named(NamedKey::Escape), true), Some(Key::Back));
        assert_eq!(map_key(&WKey::Named(NamedKey::Backspace), true), None);
        assert_eq!(map_key(&WKey::Named(NamedKey::Backspace), false), Some(Key::Back));
    }
}
