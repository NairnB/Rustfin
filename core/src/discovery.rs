//! LAN server discovery (SPEC §5, R-DISC-*). Signatures fixed by the lead; bodies are packet C2.
use serde::Deserialize;
use std::collections::HashMap;
use std::net::{SocketAddr, UdpSocket};
use std::time::{Duration, Instant};

pub const DISCOVERY_PORT: u16 = 7359;
pub const DISCOVERY_MESSAGE: &[u8] = b"Who is JellyfinServer?";

#[derive(Deserialize, Debug, Clone, PartialEq, Default)]
#[serde(rename_all = "PascalCase", default)]
pub struct DiscoveredServer {
    pub id: String,
    pub name: String,
    /// e.g. "http://172.40.0.3:8096"
    pub address: String,
    pub endpoint_address: Option<String>,
}

/// Parses one UDP reply; None for anything that isn't a server reply (missing Id or Address).
pub fn parse_reply(bytes: &[u8]) -> Option<DiscoveredServer> {
    let s: DiscoveredServer = serde_json::from_slice(bytes).ok()?;
    if s.id.is_empty() || s.address.is_empty() {
        return None;
    }
    Some(s)
}

/// R-DISC-1/2: broadcast to 255.255.255.255:7359, collect for `wait`, dedupe by id, sort by name.
/// Any socket error → empty list (never an error).
pub fn discover(wait: Duration) -> Vec<DiscoveredServer> {
    let socket = match UdpSocket::bind("0.0.0.0:0") {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    if socket.set_broadcast(true).is_err() {
        return Vec::new();
    }
    let target = SocketAddr::from(([255, 255, 255, 255], DISCOVERY_PORT));
    if socket.send_to(DISCOVERY_MESSAGE, target).is_err() {
        return Vec::new();
    }
    collect(&socket, wait)
}

/// Same, but sent to one address (unicast) — used by tests and "probe this host".
pub fn discover_at(target: SocketAddr, wait: Duration) -> Vec<DiscoveredServer> {
    let socket = match UdpSocket::bind("0.0.0.0:0") {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let _ = socket.set_broadcast(true);
    if socket.send_to(DISCOVERY_MESSAGE, target).is_err() {
        return Vec::new();
    }
    collect(&socket, wait)
}

/// Read replies from an already-bound socket until `wait` has elapsed; dedupe by `Id`, sort by `Name`.
fn collect(socket: &UdpSocket, wait: Duration) -> Vec<DiscoveredServer> {
    let deadline = Instant::now() + wait;
    let mut buf = [0u8; 2048];
    let mut by_id: HashMap<String, DiscoveredServer> = HashMap::new();
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        if socket.set_read_timeout(Some(remaining.min(Duration::from_millis(200)))).is_err() {
            break;
        }
        if let Ok((n, _)) = socket.recv_from(&mut buf) {
            if let Some(s) = parse_reply(&buf[..n]) {
                by_id.entry(s.id.clone()).or_insert(s);
            }
        }
    }
    let mut servers: Vec<DiscoveredServer> = by_id.into_values().collect();
    servers.sort_by(|a, b| a.name.cmp(&b.name));
    servers
}
