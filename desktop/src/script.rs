//! Scripted runs for render tests and benchmarks (SPEC §11 render, R-PERF-1). One command per line:
//!   wait-screen <name>     wait until the top screen is <name> (60 s timeout → exit 4: slack for a busy build box)
//!   idle                   wait until no loads are pending (then 3 more frames)
//!   key <up|down|left|right|enter|back|esc|space|backspace|info|forward|search> [n]
//!                          (esc = Back, like the real Esc key; n > 1 = the key held down: a held Left never
//!                          leaves the screen for the sidebar)
//!   type <text>            text into the focused field
//!   wait <ms>
//!   fast-forward <secs>    move the app clock forward (idle timers: the screensaver)
//!   shot <name>            screenshot to <shots dir>/<name>.png
//!   mark <label>           print "MARK <label> <ms since start>"
//!   bench-keys <key> <n>   press <key> once per frame for n frames; print frame-time stats
//!   mem                    print resident memory (Linux)
//!   quit
//! Mouse (x, y in screenshot pixels, top-left = 0 0):
//!   move <x> <y>           move the pointer there (hover)
//!   click <x> <y>          move, press, release the left button (three frames)
//!   scroll <dx> <dy>       mouse wheel at the pointer, in pixels (dy < 0 = scroll down the page, like a wheel)
//!   mouse <back|forward>   the mouse side buttons 4 / 5 (same path as a real mouse)
//!   drag <x1> <y1> <x2> <y2> <ms>   press at x1 y1, move to x2 y2 over ms (one move per frame), release (a flick
//!                          when short)
//! Video:
//!   record <name> [fps]    from now on every frame is exactly 1/fps s of app time (default 30) and is saved as
//!                          <shots dir>/<name>/00001.png …; `wait` and `drag` count app time while recording
//!   record-stop
//! `--drive <dir>` (rf-drive): the same commands, one line at a time from the FIFO `<dir>/in`; see `Drive`.
use jf_core::platform::Key;
use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub enum Cmd {
    WaitScreen(String),
    Idle,
    Key(Key, usize),
    Type(String),
    Wait(u64),
    FastForward(f64),
    Shot(String),
    Mark(String),
    BenchKeys(Key, usize),
    Mem,
    Quit,
    Mouse(Mouse),
    /// One pointer move per frame along a line, timed on the app clock.
    Glide {
        from: (f32, f32),
        to: (f32, f32),
        ms: u64,
    },
    Record(String, u32),
    RecordStop,
    /// Drive mode: like `Idle` but gives up after 10 s, silently.
    DriveSettle,
    /// Drive mode end of a command: shoot step <n> (an error message makes it an error line); started at.
    DriveShot(u32, Instant, Option<String>),
}

/// A pointer action for one frame (`click` expands to Move, Press, Release).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Mouse {
    Move(f32, f32),
    Press,
    Release,
    Scroll(f32, f32),
    Back,
    Forward,
}

pub fn parse(src: &str) -> Result<Vec<Cmd>, String> {
    let mut v = vec![];
    for (n, line) in src.lines().enumerate() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let (cmd, rest) = line.split_once(' ').unwrap_or((line, ""));
        let rest = rest.trim();
        let bad = || format!("script line {}: {line}", n + 1);
        let c = match cmd {
            "wait-screen" => Cmd::WaitScreen(rest.into()),
            "idle" => Cmd::Idle,
            "key" => {
                let mut p = rest.split_whitespace();
                let k = key(p.next().unwrap_or("")).ok_or_else(bad)?;
                Cmd::Key(k, p.next().and_then(|x| x.parse().ok()).unwrap_or(1))
            }
            "type" => Cmd::Type(rest.into()),
            "wait" => Cmd::Wait(rest.parse().map_err(|_| bad())?),
            "fast-forward" => Cmd::FastForward(rest.parse().map_err(|_| bad())?),
            "shot" => Cmd::Shot(rest.into()),
            "mark" => Cmd::Mark(rest.into()),
            "bench-keys" => {
                let mut p = rest.split_whitespace();
                let k = key(p.next().unwrap_or("")).ok_or_else(bad)?;
                Cmd::BenchKeys(k, p.next().and_then(|x| x.parse().ok()).ok_or_else(bad)?)
            }
            "mem" => Cmd::Mem,
            "quit" => Cmd::Quit,
            "move" | "click" | "scroll" => {
                let n: Vec<f32> = rest.split_whitespace().filter_map(|x| x.parse().ok()).collect();
                let [x, y] = n[..] else { return Err(bad()) };
                match cmd {
                    "move" => Cmd::Mouse(Mouse::Move(x, y)),
                    "scroll" => Cmd::Mouse(Mouse::Scroll(x, y)),
                    _ => {
                        v.push(Cmd::Mouse(Mouse::Move(x, y)));
                        v.push(Cmd::Mouse(Mouse::Press));
                        Cmd::Mouse(Mouse::Release)
                    }
                }
            }
            "drag" => {
                let n: Vec<f32> = rest.split_whitespace().filter_map(|x| x.parse().ok()).collect();
                let [x1, y1, x2, y2, ms] = n[..] else { return Err(bad()) };
                v.push(Cmd::Mouse(Mouse::Move(x1, y1)));
                v.push(Cmd::Mouse(Mouse::Press));
                v.push(Cmd::Glide { from: (x1, y1), to: (x2, y2), ms: ms as u64 });
                Cmd::Mouse(Mouse::Release)
            }
            "record" => {
                let mut p = rest.split_whitespace();
                let name = p.next().ok_or_else(bad)?.to_string();
                Cmd::Record(name, p.next().and_then(|x| x.parse().ok()).unwrap_or(30))
            }
            "record-stop" => Cmd::RecordStop,
            "mouse" => match rest {
                "back" => Cmd::Mouse(Mouse::Back),
                "forward" => Cmd::Mouse(Mouse::Forward),
                _ => return Err(bad()),
            },
            _ => return Err(bad()),
        };
        v.push(c);
    }
    Ok(v)
}

/// The only keys a TV remote has (drive mode, `--tv`).
const TV_KEYS: [&str; 8] = ["up", "down", "left", "right", "enter", "back", "info", "playpause"];

/// One drive-mode line from the tester. Desk: click/move/scroll/drag/mouse/key/type/wait/shot; TV (`tv_only`): `key` with
/// a remote key, and `wait`. Anything else is an error (never a crash). Key names are case-blind; `playpause` = `space`.
pub fn parse_drive(line: &str, tv_only: bool) -> Result<Vec<Cmd>, String> {
    let line = line.trim();
    let (cmd, rest) = line.split_once(' ').unwrap_or((line, ""));
    let (lc, rest) = (cmd.to_lowercase(), rest.trim());
    match lc.as_str() {
        "wait" => match rest.parse::<u64>() {
            Ok(ms) if ms <= 10_000 => Ok(vec![Cmd::Wait(ms)]),
            _ => Err("wait takes milliseconds, at most 10000".into()),
        },
        "key" => {
            let name = rest.split_whitespace().next().unwrap_or("").to_lowercase();
            if tv_only && !TV_KEYS.contains(&name.as_str()) {
                return Err("not available on a TV remote".into());
            }
            let name = if name == "playpause" { "space" } else { &name };
            let n = rest.split_whitespace().nth(1).unwrap_or("");
            parse(&format!("key {name} {n}")).map_err(|_| format!("unknown key: {rest}"))
        }
        _ if tv_only && matches!(lc.as_str(), "click" | "move" | "scroll" | "drag" | "mouse" | "type" | "shot") => {
            Err("not available on a TV remote".into())
        }
        // Typed text is taken as-is (the tour parser would cut it at a `#`).
        "type" => Ok(vec![Cmd::Type(rest.into())]),
        // Every step ends with a shot anyway.
        "shot" => Ok(vec![]),
        "click" | "move" | "scroll" | "drag" | "mouse" => {
            parse(&format!("{lc} {rest}")).map_err(|_| format!("bad arguments: {line}"))
        }
        _ if tv_only => Err("not available on a TV remote".into()),
        _ => Err(format!("unknown command: {cmd}")),
    }
}

fn key(s: &str) -> Option<Key> {
    Some(match s {
        "up" => Key::Up,
        "down" => Key::Down,
        "left" => Key::Left,
        "right" => Key::Right,
        "enter" => Key::Enter,
        "back" | "esc" => Key::Back,
        "forward" => Key::Forward,
        "search" => Key::Search,
        "space" => Key::PlayPause,
        "backspace" => Key::Backspace,
        "info" => Key::Info,
        "fullscreen" => Key::Fullscreen,
        "mute" => Key::Mute,
        "next" => Key::Next,
        "previous" => Key::Previous,
        "chapters" => Key::Chapters,
        "stats" => Key::Stats,
        "queue" => Key::Queue,
        "delete" => Key::Delete,
        "volumeup" => Key::VolumeUp,
        "volumedown" => Key::VolumeDown,
        _ => return None,
    })
}

/// What the runner wants from this frame.
#[derive(Default)]
pub struct Step {
    pub keys: Vec<Key>,
    pub text: Option<String>,
    pub shot: Option<String>,
    pub fast_forward: Option<f64>,
    pub mouse: Option<Mouse>,
    /// The keys are a held key's repeats (`key <k> <n>` with n > 1).
    pub held: bool,
    /// Start (`Some(Some((name, fps)))`) or stop (`Some(None)`) recording.
    pub record: Option<Option<(String, u32)>>,
    pub quit: bool,
    /// Drive mode: take the step's screenshot now (step number, when it started, error to report).
    pub drive_shot: Option<(u32, Instant, Option<String>)>,
}

/// `--drive <dir>`: commands arrive one per line on the FIFO `<dir>/in` (the wrapper makes it; a reader thread reopens
/// it at EOF). Every line becomes its commands + a settle + a screenshot, numbered 1, 2, … (`main` writes the shot and
/// the `out` line). Control lines: `!ready` restarts the numbering and, with `--tv`, locks to remote keys (the
/// wrapper's sign-in prelude runs before it); `!quit` ends the app. Testers can't send `!` lines (unknown command).
struct Drive {
    rx: mpsc::Receiver<String>,
    /// Tests queue lines through this instead of the FIFO.
    #[cfg(test)]
    tx: mpsc::Sender<String>,
    tv: bool,
    restricted: bool,
    n: u32,
}

pub struct Runner {
    cmds: Vec<Cmd>,
    pc: usize,
    since: Instant,
    idle_frames: usize,
    bench: Vec<f32>,
    /// Inside a `key <k> <n>` burst with n > 1.
    burst: bool,
    pub started: Instant,
    /// App clock (s) when the current command started; `wait` and `drag` measure on it.
    since_clock: f64,
    drive: Option<Drive>,
}

impl Runner {
    pub fn new(cmds: Vec<Cmd>, started: Instant) -> Runner {
        Runner {
            cmds,
            pc: 0,
            since: Instant::now(),
            idle_frames: 0,
            bench: vec![],
            burst: false,
            started,
            since_clock: 0.0,
            drive: None,
        }
    }

    /// Drive mode: no script; wait for lines on `<dir>/in`. Shots go to `<dir>/shots`.
    pub fn drive(dir: &Path, tv: bool, started: Instant) -> Runner {
        let _ = std::fs::create_dir_all(dir.join("shots"));
        let (tx, rx) = mpsc::channel();
        let fifo = dir.join("in");
        #[cfg(test)]
        let t2 = tx.clone();
        #[cfg(not(test))]
        let t2 = tx;
        std::thread::spawn(move || read_fifo(&fifo, t2));
        let mut r = Runner::new(vec![], started);
        r.drive = Some(Drive {
            rx,
            #[cfg(test)]
            tx,
            tv,
            restricted: false,
            n: 0,
        });
        r
    }

    /// Queue one drive line as if it had arrived on the FIFO (tests).
    #[cfg(test)]
    fn feed(&self, line: &str) {
        let _ = self.drive.as_ref().unwrap().tx.send(line.into());
    }

    /// Drive mode with nothing queued: take the next line, if one has arrived.
    fn pull(&mut self, quit: &mut bool) {
        let Some(d) = self.drive.as_mut() else { return };
        let Ok(line) = d.rx.try_recv() else { return };
        self.cmds.clear();
        self.pc = 0;
        match line.trim() {
            "!ready" => (d.n, d.restricted) = (0, d.tv),
            "!quit" => *quit = true,
            l => {
                d.n += 1;
                let (cmds, err) = match parse_drive(l, d.restricted) {
                    Ok(c) => (c, None),
                    Err(e) => (vec![], Some(e)),
                };
                self.cmds = cmds;
                // 200 ms of app time lets a screen change finish its slide before the shot.
                self.cmds.extend([Cmd::DriveSettle, Cmd::Wait(200), Cmd::DriveShot(d.n, Instant::now(), err)]);
            }
        }
    }

    /// The current step acts on the next frame (keys, text, shots, bench…, or `idle` once nothing is loading);
    /// only waits can sleep between frames.
    pub fn acts_now(&self, app_idle: bool) -> bool {
        match self.cmds.get(self.pc) {
            Some(Cmd::Idle | Cmd::DriveSettle) => app_idle,
            Some(Cmd::WaitScreen(_) | Cmd::Wait(_)) => false,
            None => self.drive.is_none(),
            _ => true,
        }
    }

    /// Called once per frame with the app's state and clock (seconds); returns what to do this frame.
    pub fn step(&mut self, screen: &str, idle: bool, frame_ms: f32, clock: f64) -> Step {
        let mut s = Step::default();
        if self.pc >= self.cmds.len() {
            self.pull(&mut s.quit);
        }
        let Some(cmd) = self.cmds.get(self.pc).cloned() else {
            s.quit |= self.drive.is_none();
            return s;
        };
        let elapsed = self.since.elapsed();
        let mut next = true;
        match cmd {
            Cmd::WaitScreen(name) => {
                if screen != name {
                    next = false;
                    if elapsed > Duration::from_secs(60) {
                        eprintln!("SCRIPT FAIL: waited 60 s for screen {name}, still on {screen}");
                        std::process::exit(4);
                    }
                }
            }
            Cmd::Idle => {
                self.idle_frames = if idle { self.idle_frames + 1 } else { 0 };
                next = self.idle_frames >= 3 || elapsed > Duration::from_secs(30);
                if next && self.idle_frames < 3 {
                    eprintln!("SCRIPT WARN: not idle after 30 s on {screen}");
                }
            }
            Cmd::DriveSettle => {
                self.idle_frames = if idle { self.idle_frames + 1 } else { 0 };
                next = self.idle_frames >= 3 || elapsed > Duration::from_secs(10);
            }
            Cmd::DriveShot(n, at, err) => s.drive_shot = Some((n, at, err)),
            Cmd::Key(k, n) => {
                s.keys = vec![k];
                self.burst |= n > 1;
                s.held = self.burst;
                if n > 1 {
                    self.cmds[self.pc] = Cmd::Key(k, n - 1);
                    next = false;
                }
            }
            Cmd::Type(t) => s.text = Some(t),
            Cmd::Wait(ms) => next = (clock - self.since_clock) * 1000.0 >= ms as f64,
            Cmd::FastForward(secs) => s.fast_forward = Some(secs),
            Cmd::Shot(name) => s.shot = Some(name),
            Cmd::Mark(label) => println!("MARK {label} {}", self.started.elapsed().as_millis()),
            Cmd::BenchKeys(k, n) => {
                if !self.bench.is_empty() || n > 0 {
                    if self.since.elapsed() > Duration::ZERO && n > 0 {
                        s.keys = vec![k];
                    }
                    if self.bench.len() < n + 1 {
                        self.bench.push(frame_ms);
                        next = false;
                    } else {
                        let mut v: Vec<f32> = self.bench.drain(1..).collect();
                        self.bench.clear();
                        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
                        let avg = v.iter().sum::<f32>() / v.len() as f32;
                        println!(
                            "BENCH frames={} avg_ms={avg:.2} p95_ms={:.2} max_ms={:.2} fps={:.0}",
                            v.len(),
                            v[v.len() * 95 / 100],
                            v[v.len() - 1],
                            1000.0 / avg
                        );
                    }
                }
            }
            Cmd::Mem => println!("MEM rss_mb={:.1}", rss_mb()),
            Cmd::Mouse(m) => s.mouse = Some(m),
            Cmd::Glide { from, to, ms } => {
                let k = (((clock - self.since_clock) * 1000.0) / ms.max(1) as f64).min(1.0) as f32;
                s.mouse = Some(Mouse::Move(from.0 + (to.0 - from.0) * k, from.1 + (to.1 - from.1) * k));
                next = k >= 1.0;
            }
            Cmd::Record(name, fps) => s.record = Some(Some((name, fps))),
            Cmd::RecordStop => s.record = Some(None),
            Cmd::Quit => s.quit = true,
        }
        if next {
            if !matches!(self.cmds.get(self.pc), Some(Cmd::Key(_, n)) if *n > 1) {
                self.burst = false;
            }
            self.pc += 1;
            self.since = Instant::now();
            self.since_clock = clock;
            self.idle_frames = 0;
        }
        s
    }
}

/// Feed the FIFO's lines to the runner; at EOF (the writer closed) open it again.
fn read_fifo(path: &PathBuf, tx: mpsc::Sender<String>) {
    loop {
        if let Ok(f) = std::fs::File::open(path) {
            for line in std::io::BufReader::new(f).lines().map_while(Result::ok) {
                let _ = tx.send(line);
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

pub fn rss_mb() -> f64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| s.lines().find(|l| l.starts_with("VmRSS:")).map(|l| l.to_string()))
        .and_then(|l| l.split_whitespace().nth(1).and_then(|k| k.parse::<f64>().ok()))
        .map(|kb| kb / 1024.0)
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mouse_commands_parse() {
        let v = parse("click 10 20\nmove 1 2\nscroll 0 -300\nmouse back\nmouse forward\nkey esc").unwrap();
        let m: Vec<String> = v.iter().map(|c| format!("{c:?}")).collect();
        assert_eq!(m[0], "Mouse(Move(10.0, 20.0))");
        assert_eq!(m[1], "Mouse(Press)");
        assert_eq!(m[2], "Mouse(Release)");
        assert_eq!(m[3], "Mouse(Move(1.0, 2.0))");
        assert_eq!(m[4], "Mouse(Scroll(0.0, -300.0))");
        assert_eq!(m[5], "Mouse(Back)");
        assert_eq!(m[6], "Mouse(Forward)");
        assert_eq!(m[7], "Key(Back, 1)");
        assert!(parse("click 10").is_err());
        assert!(parse("mouse middle").is_err());
    }

    #[test]
    fn drag_and_record_parse() {
        let v = parse("drag 100 500 900 500 120\nrecord clip\nrecord-stop").unwrap();
        let m: Vec<String> = v.iter().map(|c| format!("{c:?}")).collect();
        assert_eq!(m[0], "Mouse(Move(100.0, 500.0))");
        assert_eq!(m[1], "Mouse(Press)");
        assert_eq!(m[2], "Glide { from: (100.0, 500.0), to: (900.0, 500.0), ms: 120 }");
        assert_eq!(m[3], "Mouse(Release)");
        assert_eq!(m[4], "Record(\"clip\", 30)");
        assert_eq!(m[5], "RecordStop");
        assert!(parse("drag 1 2 3").is_err());
    }

    #[test]
    fn glide_moves_on_the_app_clock() {
        let mut r = Runner::new(parse("drag 0 0 100 0 100").unwrap(), Instant::now());
        r.step("x", true, 16.0, 0.0);
        r.step("x", true, 16.0, 0.0);
        let s = r.step("x", true, 16.0, 0.05);
        assert_eq!(s.mouse, Some(Mouse::Move(50.0, 0.0)));
        let s = r.step("x", true, 16.0, 0.1);
        assert_eq!(s.mouse, Some(Mouse::Move(100.0, 0.0)));
        assert_eq!(r.step("x", true, 16.0, 0.1).mouse, Some(Mouse::Release));
    }

    #[test]
    fn drive_commands_parse_and_validate() {
        let d = |l: &str, tv| parse_drive(l, tv);
        // Desk takes every tour command a tester needs; key names are case-blind, playpause = space.
        assert_eq!(format!("{:?}", d("click 10 20", false).unwrap()[2]), "Mouse(Release)");
        assert_eq!(format!("{:?}", d("Key Enter", false).unwrap()), "[Key(Enter, 1)]");
        assert_eq!(format!("{:?}", d("key PlayPause", false).unwrap()), "[Key(PlayPause, 1)]");
        assert_eq!(format!("{:?}", d("type Hello #1", false).unwrap()), "[Type(\"Hello #1\")]");
        assert!(d("shot", false).unwrap().is_empty());
        assert!(d("wait 500", false).is_ok());
        assert!(d("wait 99999", false).is_err());
        // Unknown commands and tour-only ones are errors, not crashes.
        for bad in ["dance", "quit", "!quit", "record x", "fast-forward 5", "wait-screen home", "key nope", "click 1"] {
            assert!(d(bad, false).is_err(), "{bad}");
        }
        // TV remote: only the eight keys and wait.
        for k in ["Up", "Down", "Left", "Right", "Enter", "Back", "Info", "PlayPause"] {
            assert!(d(&format!("key {k}"), true).is_ok(), "{k}");
        }
        assert!(d("wait 300", true).is_ok());
        for bad in ["click 1 2", "move 1 2", "scroll 0 5", "type hi", "key esc", "key backspace", "key search", "shot"]
        {
            assert_eq!(d(bad, true).unwrap_err(), "not available on a TV remote", "{bad}");
        }
    }

    #[test]
    fn drive_runner_numbers_steps_and_locks_tv_after_ready() {
        let dir = std::env::temp_dir().join(format!("rfd-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut r = Runner::drive(&dir, true, Instant::now());
        r.feed("click 5 5"); // prelude: unrestricted, still numbered
        r.feed("!ready");
        r.feed("click 1 1");
        r.feed("key Down");
        let mut shots = vec![];
        for i in 0..200 {
            let s = r.step("home", true, 16.0, i as f64);
            assert!(!s.quit);
            shots.extend(s.drive_shot);
        }
        let got: Vec<(u32, Option<String>)> = shots.into_iter().map(|(n, _, e)| (n, e)).collect();
        assert_eq!(got[0], (1, None));
        assert_eq!(got[1].0, 1);
        assert_eq!(got[1].1.as_deref(), Some("not available on a TV remote"));
        assert_eq!(got[2], (2, None));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
