//! Runtime side of the image pipeline (R-IMG-1/2): disk cache → network → decode on workers → GPU texture.
//! Screens call `get` for what they draw (and `prefetch` one row ahead); queued requests nobody asked for this
//! frame are dropped before they start.
use crate::api::Client;
use crate::images::{cache_key, DiskCache, Lru};
use crate::tasks::Pool;
use crate::time::Instant;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

const MAX_INFLIGHT: usize = 6;
const MAX_TEXTURES: usize = 1500;
/// PP review #2: a remote poster fetched at full original size is downscaled once to this width and the smaller
/// image cached, so a 3000 px TheMovieDb/Fanart original isn't decoded for every card.
const MAX_TEX_W: u32 = 1280;
/// C3-POLISH: an image still not loaded this long after it was first asked for is given up on by screens that can
/// show something else instead (a playlist tile re-lays out its other posters). It still loads, and shows once it has.
pub const STALL: std::time::Duration = std::time::Duration::from_secs(6);

type Decoded = (String, Option<egui::ColorImage>);

pub struct Images {
    pool: Pool,
    disk: Option<Arc<DiskCache>>,
    client: Option<Client>,
    /// `file://` images are read only from under these folders (the poster store, a tour's fixture dir).
    file_roots: Arc<Vec<std::path::PathBuf>>,
    textures: Lru<String, egui::TextureHandle>,
    queue: VecDeque<String>,
    inflight: HashSet<String>,
    failed: HashSet<String>,
    asked: HashSet<String>,
    /// When each image not loaded yet was first asked for (on screen without a break; see `STALL`).
    waiting: HashMap<String, Instant>,
    /// Render tours only (`image-stub`): URLs containing one of these fail, or never load.
    stub_fail: Vec<String>,
    stub_stall: Vec<String>,
    tx: Sender<Decoded>,
    rx: Receiver<Decoded>,
    pub loaded_total: usize,
    pub first_loaded_at: Option<Instant>,
}

impl Images {
    pub fn new(disk: Option<DiskCache>) -> Images {
        let (tx, rx) = channel();
        Images {
            pool: Pool::new(4),
            disk: disk.map(Arc::new),
            client: None,
            file_roots: Arc::new(vec![]),
            textures: Lru::new(MAX_TEXTURES),
            queue: VecDeque::new(),
            inflight: HashSet::new(),
            failed: HashSet::new(),
            asked: HashSet::new(),
            waiting: HashMap::new(),
            stub_fail: vec![],
            stub_stall: vec![],
            tx,
            rx,
            loaded_total: 0,
            first_loaded_at: None,
        }
    }

    pub fn set_file_roots(&mut self, roots: Vec<std::path::PathBuf>) {
        self.file_roots = Arc::new(roots);
    }

    /// False for an image that cannot be shown: one that failed to load, or a `file://` outside the allowed
    /// folders / gone from this device. Callers fall back to the server's own image (never a blank card).
    pub fn usable(&self, url: &str) -> bool {
        if self.failed.contains(url) || stub_hit(&self.stub_fail, url) {
            return false;
        }
        !url.starts_with("file://") || crate::api::poster::file_url_allowed(url, &self.file_roots)
    }

    pub fn set_client(&mut self, c: Option<Client>) {
        self.client = c;
        self.failed.clear();
    }

    /// True while `url` is still not loaded `STALL` after it was first asked for (at `now`); false once it has a
    /// texture, failed (see `usable`), or was not asked for.
    pub fn stalled(&self, url: &str, now: Instant) -> bool {
        // (`waiting` only holds what `get` could not hand a texture for; a load removes it)
        self.waiting.get(url).is_some_and(|t| now.saturating_duration_since(*t) >= STALL)
    }

    /// Render tours (`image-stub fail|stall <substring>…`): from now on an image whose URL contains one of `pats`
    /// fails to load, or never loads (it is not queued, so tours still go idle). `clear` drops both lists.
    pub fn stub(&mut self, mode: &str, pats: &[&str]) {
        let pats = pats.iter().map(|p| p.to_string());
        match mode {
            "fail" => self.stub_fail.extend(pats),
            "stall" => self.stub_stall.extend(pats),
            _ => {
                self.stub_fail.clear();
                self.stub_stall.clear();
            }
        }
    }

    /// Texture for `url` if ready; otherwise queues it at the front (it is on screen now).
    pub fn get(&mut self, url: &str) -> Option<egui::TextureId> {
        self.asked.insert(url.to_string());
        if stub_hit(&self.stub_fail, url) {
            return None;
        }
        if stub_hit(&self.stub_stall, url) {
            self.waiting.entry(url.to_string()).or_insert_with(Instant::now);
            return None;
        }
        if let Some(t) = self.textures.get(&url.to_string()) {
            return Some(t.id());
        }
        if !self.failed.contains(url) {
            self.waiting.entry(url.to_string()).or_insert_with(Instant::now);
        }
        if self.wanted(url) {
            self.queue.push_front(url.to_string());
        }
        None
    }

    /// Queue behind on-screen work (one row ahead).
    pub fn prefetch(&mut self, url: &str) {
        self.asked.insert(url.to_string());
        if !self.textures.contains(&url.to_string()) && self.wanted(url) {
            self.queue.push_back(url.to_string());
        }
    }

    fn wanted(&self, url: &str) -> bool {
        !self.inflight.contains(url) && !self.failed.contains(url) && !self.queue.iter().any(|q| q == url)
    }

    /// Start of a frame: upload images decoded since the last frame.
    pub fn begin_frame(&mut self, ctx: &egui::Context) {
        for (url, img) in self.rx.try_iter().collect::<Vec<_>>() {
            self.inflight.remove(&url);
            self.waiting.remove(&url);
            match img {
                Some(img) => {
                    let tex = ctx.load_texture(cache_key(&url), img, egui::TextureOptions::LINEAR);
                    self.textures.insert(url, tex);
                    self.loaded_total += 1;
                    self.first_loaded_at.get_or_insert_with(Instant::now);
                }
                None => {
                    self.failed.insert(url);
                }
            }
        }
    }

    /// End of a frame: drop queued requests nobody drew or prefetched, start new ones.
    pub fn end_frame(&mut self, ctx: &egui::Context) {
        let asked = std::mem::take(&mut self.asked);
        self.queue.retain(|u| asked.contains(u));
        // off screen for a frame: the wait starts again next time
        self.waiting.retain(|u, _| asked.contains(u));
        while self.inflight.len() < MAX_INFLIGHT {
            let Some(url) = self.queue.pop_front() else { break };
            self.start(url, ctx.clone());
        }
    }

    fn start(&mut self, url: String, ctx: egui::Context) {
        let Some(client) = self.client.clone() else { return };
        let disk = self.disk.clone();
        let roots = self.file_roots.clone();
        let tx = self.tx.clone();
        self.inflight.insert(url.clone());
        self.pool.run(move || {
            let key = cache_key(&url);
            // PP-6: a `file://` poster (a personal own image) is read straight from disk, not the network.
            let bytes = if url.starts_with("file://") {
                // PP review: only under the poster store / the fixture dir.
                if crate::api::poster::file_url_allowed(&url, &roots) {
                    std::fs::read(url.trim_start_matches("file://")).ok()
                } else {
                    None
                }
            } else {
                disk.as_ref().and_then(|d| d.get(&key)).or_else(|| {
                    let b = client.get_bytes(&url).ok()?;
                    if let Some(d) = &disk {
                        let _ = d.put(&key, &b);
                    }
                    Some(b)
                })
            };
            let img = bytes.and_then(|b| image::load_from_memory(&b).ok()).map(|img| {
                // PP review #2: downscale an oversized image once and cache the smaller copy.
                let (img, resized) = if img.width() > MAX_TEX_W {
                    let h = ((img.height() as u64 * MAX_TEX_W as u64) / img.width() as u64).max(1) as u32;
                    (img.resize_exact(MAX_TEX_W, h, image::imageops::FilterType::Lanczos3), true)
                } else {
                    (img, false)
                };
                if resized {
                    if let Some(d) = &disk {
                        let mut buf = std::io::Cursor::new(vec![]);
                        if img.write_to(&mut buf, image::ImageFormat::Png).is_ok() {
                            let _ = d.put(&key, buf.get_ref());
                        }
                    }
                }
                let rgba = img.to_rgba8();
                egui::ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], &rgba)
            });
            let _ = tx.send((url, img));
            ctx.request_repaint();
        });
    }

    pub fn pending(&self) -> usize {
        self.inflight.len() + self.queue.len()
    }
}

fn stub_hit(pats: &[String], url: &str) -> bool {
    pats.iter().any(|p| url.contains(p.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_image_still_loading_after_stall_counts_as_stalled() {
        let mut im = Images::new(None);
        let t0 = Instant::now();
        assert_eq!(im.get("http://s/a"), None);
        assert!(!im.stalled("http://s/a", t0), "just asked: still loading");
        assert!(im.stalled("http://s/a", t0 + STALL + std::time::Duration::from_millis(50)));
        assert!(!im.stalled("http://s/never-asked", t0 + STALL * 2));
        // a frame where nobody drew it: the wait starts again
        im.end_frame(&egui::Context::default());
        im.end_frame(&egui::Context::default());
        assert!(!im.stalled("http://s/a", t0 + STALL * 2));
    }

    #[test]
    fn stubbed_images_fail_or_never_load_without_holding_up_idle() {
        let mut im = Images::new(None);
        im.stub("fail", &["/bad/"]);
        im.stub("stall", &["/slow/"]);
        assert_eq!(im.get("http://s/bad/1"), None);
        assert!(!im.usable("http://s/bad/1"));
        assert_eq!(im.get("http://s/slow/1"), None);
        assert!(im.usable("http://s/slow/1"));
        assert!(im.stalled("http://s/slow/1", Instant::now() + STALL));
        assert_eq!(im.pending(), 0, "nothing queued for a stubbed image");
        im.stub("clear", &[]);
        assert!(im.usable("http://s/bad/1"));
    }
}
