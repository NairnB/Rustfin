//! LAN server discovery (SPEC §5, R-DISC-*). Signatures fixed by the lead; bodies are packet C2.
use serde::Deserialize;
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::Mutex;
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

// ---- C3 (BRIEF-discovery): broadcast, then a unicast sweep of the device's own /24s ----
//
// A Jellyfin in Docker without host networking never sees a broadcast (the port publish takes unicast only), but it
// answers the same probe sent straight to its host. So: broadcast (and probe the last server's host) first; if
// nothing answers within `broadcast_wait`, probe every host of each own /24, `gap` apart, from the same socket, and
// keep reading for `tail` after the last send.

/// How a server answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Via {
    Broadcast,
    Unicast,
}

impl Via {
    pub fn label(self) -> &'static str {
        match self {
            Via::Broadcast => "broadcast",
            Via::Unicast => "unicast",
        }
    }
}

/// One server found by `search`.
#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    pub server: DiscoveredServer,
    pub via: Via,
    /// Milliseconds from the start of the search to its first reply.
    pub ms: u64,
}

/// What `search` found: de-duplicated by Id, sorted by name.
#[derive(Debug, Clone, Default)]
pub struct Report {
    pub found: Vec<Found>,
    /// Unicast probes sent (last server's host + sweep).
    pub unicast_sent: usize,
    /// Whether the /24 sweep ran.
    pub swept: bool,
    pub elapsed: Duration,
    /// Own addresses whose /24 was (or would have been) swept.
    pub subnets: Vec<Ipv4Addr>,
    /// The first socket error (bind, broadcast, send), for diagnostics; None = the socket worked.
    pub error: Option<String>,
}

impl Report {
    pub fn servers(&self) -> Vec<DiscoveredServer> {
        self.found.iter().map(|f| f.server.clone()).collect()
    }
    /// One diagnostic line (names, addresses and how; no tokens), e.g. for the TV trial report.
    pub fn line(&self) -> String {
        let list: Vec<String> = self
            .found
            .iter()
            .map(|f| format!("{} {} via {} at {} ms", f.server.name, f.server.address, f.via.label(), f.ms))
            .collect();
        let own: Vec<String> = self.subnets.iter().map(|a| a.to_string()).collect();
        format!(
            "discovery: found={} [{}] swept={} sent={} took={} ms own=[{}] error={}",
            self.found.len(),
            list.join("; "),
            self.swept,
            self.unicast_sent,
            self.elapsed.as_millis(),
            own.join(","),
            self.error.as_deref().unwrap_or("none")
        )
    }
}

/// Search settings. `Options::standard` is what the app uses; tests inject the port and the addresses.
#[derive(Debug, Clone, PartialEq)]
pub struct Options {
    /// UDP port probed (7359 for Jellyfin).
    pub port: u16,
    /// Send the broadcast first.
    pub broadcast: bool,
    /// How long to wait for any answer before sweeping.
    pub broadcast_wait: Duration,
    /// Own addresses: every host of each one's /24 is probed if nothing answered.
    pub subnets: Vec<Ipv4Addr>,
    /// Hosts probed straight away with the broadcast (the last server's host).
    pub extra_hosts: Vec<Ipv4Addr>,
    /// Pause between sweep probes (rate limit).
    pub gap: Duration,
    /// Keep reading this long after the last sweep probe.
    pub tail: Duration,
}

/// The probe port `Options::standard` uses: 7359 unless a test changed it.
static PROBE_PORT: AtomicU16 = AtomicU16::new(DISCOVERY_PORT);

/// Tests only: probe this port instead of 7359 (so fake responders need no privileged or shared port).
pub fn set_probe_port(port: u16) {
    PROBE_PORT.store(port, Ordering::Relaxed);
}

impl Options {
    /// Broadcast + the last server's host, 1 s; then the own /24s 2 ms apart; replies read 1.5 s after the last.
    pub fn standard(last_host: Option<Ipv4Addr>) -> Options {
        Options {
            port: PROBE_PORT.load(Ordering::Relaxed),
            broadcast: true,
            broadcast_wait: Duration::from_secs(1),
            subnets: own_ipv4(),
            extra_hosts: last_host.into_iter().collect(),
            gap: Duration::from_millis(2),
            tail: Duration::from_millis(1500),
        }
    }
}

/// Finds servers (see the module note). Never fails: no network → empty report. Blocking: run it on a worker.
pub fn search(o: &Options) -> Report {
    let mut report = Report { subnets: o.subnets.clone(), ..Default::default() };
    let socket = match UdpSocket::bind("0.0.0.0:0") {
        Ok(s) => s,
        Err(e) => {
            report.error = Some(format!("bind: {e}"));
            return report;
        }
    };
    let start = crate::time::Instant::now();
    let mut found: HashMap<String, Found> = HashMap::new();
    let unicast: Vec<Ipv4Addr> = o.extra_hosts.clone();
    let read = |socket: &UdpSocket, until: Duration, found: &mut HashMap<String, Found>, unicast: &[Ipv4Addr]| {
        let mut buf = [0u8; 2048];
        loop {
            let left = until.saturating_sub(start.elapsed());
            if left.is_zero() || socket.set_read_timeout(Some(left)).is_err() {
                return;
            }
            let Ok((n, from)) = socket.recv_from(&mut buf) else { continue };
            let Some(mut server) = parse_reply(&buf[..n]) else { continue };
            if let IpAddr::V4(src) = from.ip() {
                server.address = reachable_address(&server.address, src, &o.subnets);
            }
            let via = match from.ip() {
                IpAddr::V4(ip) if unicast.contains(&ip) => Via::Unicast,
                _ => Via::Broadcast,
            };
            let ms = start.elapsed().as_millis() as u64;
            found.entry(server.id.clone()).or_insert(Found { server, via, ms });
        }
    };
    if o.broadcast {
        let sent = socket
            .set_broadcast(true)
            .and_then(|_| socket.send_to(DISCOVERY_MESSAGE, SocketAddr::from((Ipv4Addr::BROADCAST, o.port))));
        if let Err(e) = sent {
            report.error.get_or_insert(format!("broadcast: {e}"));
        }
    }
    for h in &o.extra_hosts {
        match socket.send_to(DISCOVERY_MESSAGE, SocketAddr::from((*h, o.port))) {
            Ok(_) => report.unicast_sent += 1,
            Err(e) => {
                report.error.get_or_insert(format!("send {h}: {e}"));
            }
        }
    }
    read(&socket, o.broadcast_wait, &mut found, &unicast);
    if found.is_empty() {
        let mut hosts: Vec<Ipv4Addr> = vec![];
        for ip in &o.subnets {
            for h in subnet_hosts(*ip) {
                if !hosts.contains(&h) && !o.extra_hosts.contains(&h) {
                    hosts.push(h);
                }
            }
        }
        report.swept = !hosts.is_empty();
        let mut sent_to = unicast.clone();
        sent_to.extend(hosts.iter().copied());
        let sweep_start = start.elapsed();
        for (i, h) in hosts.iter().enumerate() {
            // rate limit: probe i goes out at sweep_start + i × gap; replies are read while waiting
            read(&socket, sweep_start + o.gap * i as u32, &mut found, &sent_to);
            match socket.send_to(DISCOVERY_MESSAGE, SocketAddr::from((*h, o.port))) {
                Ok(_) => report.unicast_sent += 1,
                Err(e) => {
                    report.error.get_or_insert(format!("send {h}: {e}"));
                }
            }
        }
        let end = start.elapsed() + o.tail;
        read(&socket, end, &mut found, &sent_to);
    }
    let mut list: Vec<Found> = found.into_values().collect();
    list.sort_by(|a, b| a.server.name.cmp(&b.server.name).then_with(|| a.server.id.cmp(&b.server.id)));
    report.found = list;
    report.elapsed = start.elapsed();
    report
}

/// The 254 hosts (.1 to .254) of `ip`'s /24.
pub fn subnet_hosts(ip: Ipv4Addr) -> Vec<Ipv4Addr> {
    let [a, b, c, _] = ip.octets();
    (1..=254).map(|d| Ipv4Addr::new(a, b, c, d)).collect()
}

/// A LAN address worth sweeping: private IPv4 only (10/8, 172.16/12, 192.168/16), so a public or tailnet (100.64/10)
/// address never makes the app probe strangers, and not a Docker bridge (`docker_bridge`).
pub fn usable_lan(ip: Ipv4Addr) -> bool {
    ip.is_private() && !docker_bridge(ip)
}

/// A Docker bridge address, as far as the address alone tells (std lists no interface names): Docker's default
/// bridge and user networks come from 172.17.0.0/16 to 172.31.0.0/16. Its other default pool, 192.168/16, is left
/// in because home routers use it.
pub fn docker_bridge(ip: Ipv4Addr) -> bool {
    let o = ip.octets();
    o[0] == 172 && (17..=31).contains(&o[1])
}

/// The device's own IPv4 addresses, one per /24, usable ones only. Portable without an interface API: a UDP socket
/// "connected" to an address only asks the routing table which local address would be used (nothing is sent), so
/// asking for a public address and each private range finds the default and any private-range interfaces.
pub fn own_ipv4() -> Vec<Ipv4Addr> {
    let targets: [[u8; 4]; 5] = [[8, 8, 8, 8], [192, 168, 1, 1], [192, 168, 0, 1], [10, 0, 0, 1], [172, 16, 0, 1]];
    let mut out: Vec<Ipv4Addr> = vec![];
    for t in targets {
        let Ok(s) = UdpSocket::bind("0.0.0.0:0") else { return out };
        if s.connect(SocketAddr::from((t, 9))).is_err() {
            continue;
        }
        if let Ok(SocketAddr::V4(a)) = s.local_addr() {
            let ip = *a.ip();
            if usable_lan(ip) && !out.iter().any(|o| o.octets()[..3] == ip.octets()[..3]) {
                out.push(ip);
            }
        }
    }
    out
}

/// A Jellyfin in Docker may answer with its container address (e.g. "http://172.40.0.3:8096"), which nobody else
/// can reach. When the reply's address is an IPv4 literal that is neither the host that sent the reply nor on one of
/// our own /24s, use the sender's address with the same scheme, port and path. Names are kept as they are.
pub fn reachable_address(address: &str, from: Ipv4Addr, own: &[Ipv4Addr]) -> String {
    let Some(host) = host_of(address) else { return address.to_string() };
    if host == from || own.iter().any(|o| o.octets()[..3] == host.octets()[..3]) {
        return address.to_string();
    }
    address.replacen(&host.to_string(), &from.to_string(), 1)
}

/// The IPv4 host of a server address ("http://10.0.0.20:8096" → 10.0.0.20); None for names and IPv6.
pub fn host_of(address: &str) -> Option<Ipv4Addr> {
    let rest = address.split("://").nth(1).unwrap_or(address);
    let host = rest.split(['/', ':']).next()?;
    host.parse().ok()
}

/// Where `search_lan` reports its one-line summary (the TV shell posts it to the trial report). Unset = nowhere.
static REPORTER: Mutex<Option<fn(&str)>> = Mutex::new(None);

/// Install the summary sink (a plain function: the TV build passes its diagnostics poster).
pub fn set_reporter(f: fn(&str)) {
    if let Ok(mut r) = REPORTER.lock() {
        *r = Some(f);
    }
}

/// Sends `line` to the sink set by `set_reporter`, if any.
pub fn report(line: &str) {
    let f = REPORTER.lock().ok().and_then(|r| *r);
    if let Some(f) = f {
        f(line);
    }
}

/// The app's search: `Options::standard` around the last server's host, then the summary goes to the reporter.
pub fn search_lan(last_address: Option<&str>) -> Report {
    let r = search(&Options::standard(last_address.and_then(host_of)));
    report(&r.line());
    r
}
