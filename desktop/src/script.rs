//! Scripted runs for render tests and benchmarks (SPEC §11 render, R-PERF-1). One command per line:
//!   wait-screen <name>     wait until the top screen is <name> (60 s timeout → exit 4: slack for a busy build box)
//!   idle                   wait until no loads are pending (then 3 more frames)
//!   key <up|down|left|right|enter|back|space|backspace|info> [n]
//!   type <text>            text into the focused field
//!   wait <ms>
//!   shot <name>            screenshot to <shots dir>/<name>.png
//!   mark <label>           print "MARK <label> <ms since start>"
//!   bench-keys <key> <n>   press <key> once per frame for n frames; print frame-time stats
//!   mem                    print resident memory (Linux)
//!   quit
use jf_core::platform::Key;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub enum Cmd {
    WaitScreen(String),
    Idle,
    Key(Key, usize),
    Type(String),
    Wait(u64),
    Shot(String),
    Mark(String),
    BenchKeys(Key, usize),
    Mem,
    Quit,
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
        v.push(match cmd {
            "wait-screen" => Cmd::WaitScreen(rest.into()),
            "idle" => Cmd::Idle,
            "key" => {
                let mut p = rest.split_whitespace();
                let k = key(p.next().unwrap_or("")).ok_or_else(bad)?;
                Cmd::Key(k, p.next().and_then(|x| x.parse().ok()).unwrap_or(1))
            }
            "type" => Cmd::Type(rest.into()),
            "wait" => Cmd::Wait(rest.parse().map_err(|_| bad())?),
            "shot" => Cmd::Shot(rest.into()),
            "mark" => Cmd::Mark(rest.into()),
            "bench-keys" => {
                let mut p = rest.split_whitespace();
                let k = key(p.next().unwrap_or("")).ok_or_else(bad)?;
                Cmd::BenchKeys(k, p.next().and_then(|x| x.parse().ok()).ok_or_else(bad)?)
            }
            "mem" => Cmd::Mem,
            "quit" => Cmd::Quit,
            _ => return Err(bad()),
        });
    }
    Ok(v)
}

fn key(s: &str) -> Option<Key> {
    Some(match s {
        "up" => Key::Up,
        "down" => Key::Down,
        "left" => Key::Left,
        "right" => Key::Right,
        "enter" => Key::Enter,
        "back" => Key::Back,
        "space" => Key::PlayPause,
        "backspace" => Key::Backspace,
        "info" => Key::Info,
        "fullscreen" => Key::Fullscreen,
        "mute" => Key::Mute,
        "next" => Key::Next,
        "previous" => Key::Previous,
        "chapters" => Key::Chapters,
        "stats" => Key::Stats,
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
    pub quit: bool,
}

pub struct Runner {
    cmds: Vec<Cmd>,
    pc: usize,
    since: Instant,
    idle_frames: usize,
    bench: Vec<f32>,
    pub started: Instant,
}

impl Runner {
    pub fn new(cmds: Vec<Cmd>, started: Instant) -> Runner {
        Runner { cmds, pc: 0, since: Instant::now(), idle_frames: 0, bench: vec![], started }
    }

    /// Called once per frame with the app's state; returns what to do this frame.
    pub fn step(&mut self, screen: &str, idle: bool, frame_ms: f32) -> Step {
        let mut s = Step::default();
        let Some(cmd) = self.cmds.get(self.pc).cloned() else {
            s.quit = true;
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
            Cmd::Key(k, n) => {
                s.keys = vec![k];
                if n > 1 {
                    self.cmds[self.pc] = Cmd::Key(k, n - 1);
                    next = false;
                }
            }
            Cmd::Type(t) => s.text = Some(t),
            Cmd::Wait(ms) => next = elapsed >= Duration::from_millis(ms),
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
            Cmd::Quit => s.quit = true,
        }
        if next {
            self.pc += 1;
            self.since = Instant::now();
            self.idle_frames = 0;
        }
        s
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
