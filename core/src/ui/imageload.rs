//! Runtime side of the image pipeline (R-IMG-1/2): disk cache → network → decode on workers → GPU texture.
//! Screens call `get` for what they draw (and `prefetch` one row ahead); queued requests nobody asked for this
//! frame are dropped before they start.
use crate::api::Client;
use crate::images::{cache_key, DiskCache, Lru};
use crate::tasks::Pool;
use crate::time::Instant;
use std::collections::{HashSet, VecDeque};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

const MAX_INFLIGHT: usize = 6;
const MAX_TEXTURES: usize = 1500;

type Decoded = (String, Option<egui::ColorImage>);

pub struct Images {
    pool: Pool,
    disk: Option<Arc<DiskCache>>,
    client: Option<Client>,
    textures: Lru<String, egui::TextureHandle>,
    queue: VecDeque<String>,
    inflight: HashSet<String>,
    failed: HashSet<String>,
    asked: HashSet<String>,
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
            textures: Lru::new(MAX_TEXTURES),
            queue: VecDeque::new(),
            inflight: HashSet::new(),
            failed: HashSet::new(),
            asked: HashSet::new(),
            tx,
            rx,
            loaded_total: 0,
            first_loaded_at: None,
        }
    }

    pub fn set_client(&mut self, c: Option<Client>) {
        self.client = c;
        self.failed.clear();
    }

    /// Texture for `url` if ready; otherwise queues it at the front (it is on screen now).
    pub fn get(&mut self, url: &str) -> Option<egui::TextureId> {
        self.asked.insert(url.to_string());
        if let Some(t) = self.textures.get(&url.to_string()) {
            return Some(t.id());
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
        while self.inflight.len() < MAX_INFLIGHT {
            let Some(url) = self.queue.pop_front() else { break };
            self.start(url, ctx.clone());
        }
    }

    fn start(&mut self, url: String, ctx: egui::Context) {
        let Some(client) = self.client.clone() else { return };
        let disk = self.disk.clone();
        let tx = self.tx.clone();
        self.inflight.insert(url.clone());
        self.pool.run(move || {
            let key = cache_key(&url);
            let bytes = disk.as_ref().and_then(|d| d.get(&key)).or_else(|| {
                let b = client.get_bytes(&url).ok()?;
                if let Some(d) = &disk {
                    let _ = d.put(&key, &b);
                }
                Some(b)
            });
            let img = bytes.and_then(|b| image::load_from_memory(&b).ok()).map(|img| {
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
