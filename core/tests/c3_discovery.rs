//! C3 discovery (BRIEF-discovery): a server in Docker answers only unicast, so a silent broadcast is followed by a
//! rate-limited unicast sweep of the device's own /24s plus the last server's host. Fake responders bind one
//! 127.0.0.x address on a test port: a socket bound to one address never sees broadcasts, so they answer unicast only.
use jf_core::discovery::*;
use std::net::{Ipv4Addr, UdpSocket};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// A fake Jellyfin that answers the probe on `ip:port` with `id`/`name`; counts the probes it got.
fn responder(ip: [u8; 4], port: u16, id: &str, name: &str) -> Arc<AtomicUsize> {
    let address = format!("http://{}.{}.{}.{}:8096", ip[0], ip[1], ip[2], ip[3]);
    responder_saying(ip, port, id, name, &address)
}

/// Same, but the reply carries `address` (e.g. a Docker container address).
fn responder_saying(ip: [u8; 4], port: u16, id: &str, name: &str, address: &str) -> Arc<AtomicUsize> {
    let sock = UdpSocket::bind((Ipv4Addr::from(ip), port)).expect("bind responder");
    sock.set_read_timeout(Some(Duration::from_secs(8))).unwrap();
    let hits = Arc::new(AtomicUsize::new(0));
    let h = hits.clone();
    let reply = format!(r#"{{"Address":"{address}","Id":"{id}","Name":"{name}","EndpointAddress":null}}"#);
    std::thread::spawn(move || {
        let mut buf = [0u8; 256];
        while let Ok((n, from)) = sock.recv_from(&mut buf) {
            if &buf[..n] == DISCOVERY_MESSAGE {
                h.fetch_add(1, Ordering::SeqCst);
                let _ = sock.send_to(reply.as_bytes(), from);
            }
        }
    });
    hits
}

/// A port nobody else uses in this test binary (each test gets its own).
fn port(n: u16) -> u16 {
    40000 + (std::process::id() % 2000) as u16 * 10 + n
}

fn opts(port: u16, subnets: Vec<Ipv4Addr>, extra: Vec<Ipv4Addr>) -> Options {
    Options {
        port,
        broadcast: true,
        broadcast_wait: Duration::from_millis(300),
        subnets,
        extra_hosts: extra,
        gap: Duration::from_millis(1),
        tail: Duration::from_millis(400),
    }
}

#[test]
fn unicast_only_server_is_found_by_the_sweep() {
    let p = port(1);
    let hits = responder([127, 0, 0, 5], p, "srv-a", "Docker Jellyfin");
    let r = search(&opts(p, vec![Ipv4Addr::new(127, 0, 0, 1)], vec![]));
    assert_eq!(r.found.len(), 1, "{:?}", r.found);
    assert_eq!(r.found[0].server.id, "srv-a");
    assert_eq!(r.found[0].server.address, "http://127.0.0.5:8096");
    assert_eq!(r.found[0].via, Via::Unicast);
    assert_eq!(hits.load(Ordering::SeqCst), 1, "one probe per host");
    assert_eq!(r.unicast_sent, 254);
}

#[test]
fn duplicates_by_id_are_merged_and_sorted_by_name() {
    let p = port(2);
    responder([127, 0, 0, 20], p, "same", "Zed");
    responder([127, 0, 0, 21], p, "same", "Zed");
    responder([127, 0, 0, 22], p, "other", "Alpha");
    let r = search(&opts(p, vec![Ipv4Addr::new(127, 0, 0, 9)], vec![]));
    let names: Vec<&str> = r.found.iter().map(|f| f.server.name.as_str()).collect();
    assert_eq!(names, ["Alpha", "Zed"], "{:?}", r.found);
}

#[test]
fn last_server_host_is_probed_even_outside_the_subnets() {
    let p = port(3);
    responder([127, 0, 1, 7], p, "last", "Old friend");
    let t = Instant::now();
    // no subnet at all: only the last server's host
    let r = search(&opts(p, vec![], vec![Ipv4Addr::new(127, 0, 1, 7)]));
    assert_eq!(r.found.len(), 1, "{:?}", r.found);
    assert_eq!(r.found[0].server.id, "last");
    assert_eq!(r.found[0].via, Via::Unicast);
    assert!(t.elapsed() < Duration::from_secs(2), "the last host answers in the first phase: {:?}", t.elapsed());
}

#[test]
fn a_reply_in_the_first_phase_skips_the_sweep() {
    let p = port(4);
    responder([127, 0, 2, 7], p, "last", "Quick");
    let r = search(&opts(p, vec![Ipv4Addr::new(127, 0, 3, 1)], vec![Ipv4Addr::new(127, 0, 2, 7)]));
    assert_eq!(r.found.len(), 1);
    assert_eq!(r.unicast_sent, 1, "only the last host: nothing to sweep once something answered");
}

#[test]
fn sweep_is_rate_limited() {
    let p = port(5);
    let mut o = opts(p, vec![Ipv4Addr::new(127, 0, 4, 1)], vec![]);
    o.broadcast = false;
    o.broadcast_wait = Duration::ZERO;
    o.gap = Duration::from_millis(2);
    o.tail = Duration::ZERO;
    let t = Instant::now();
    let r = search(&o);
    let took = t.elapsed();
    assert_eq!(r.unicast_sent, 254);
    assert!(took >= Duration::from_millis(2 * 253), "254 probes 2 ms apart take ≥ 506 ms, took {took:?}");
    assert!(took < Duration::from_millis(3000), "{took:?}");
}

#[test]
fn nothing_there_is_an_empty_list_in_bounded_time() {
    let p = port(6);
    let t = Instant::now();
    let r = search(&opts(p, vec![Ipv4Addr::new(127, 0, 5, 1)], vec![]));
    assert!(r.found.is_empty());
    assert!(t.elapsed() < Duration::from_secs(3), "{:?}", t.elapsed());
}

#[test]
fn subnet_hosts_are_the_254_of_the_slash_24() {
    let h = subnet_hosts(Ipv4Addr::new(192, 168, 7, 42));
    assert_eq!(h.len(), 254);
    assert_eq!(h[0], Ipv4Addr::new(192, 168, 7, 1));
    assert_eq!(h[253], Ipv4Addr::new(192, 168, 7, 254));
    assert!(h.iter().all(|a| a.octets()[..3] == [192, 168, 7]));
}

#[test]
fn own_addresses_skip_loopback_link_local_and_docker() {
    assert!(usable_lan(Ipv4Addr::new(192, 168, 1, 30)));
    assert!(usable_lan(Ipv4Addr::new(10, 0, 0, 4)));
    assert!(!usable_lan(Ipv4Addr::new(127, 0, 0, 1)));
    assert!(!usable_lan(Ipv4Addr::new(169, 254, 3, 3)));
    assert!(!usable_lan(Ipv4Addr::new(172, 17, 0, 2)), "Docker's default bridge");
    assert!(!usable_lan(Ipv4Addr::UNSPECIFIED));
    assert!(usable_lan(Ipv4Addr::new(172, 16, 5, 5)));
    assert!(!usable_lan(Ipv4Addr::new(100, 101, 71, 54)), "tailnet");
    assert!(!usable_lan(Ipv4Addr::new(51, 254, 199, 33)), "public");
    assert!(!usable_lan(Ipv4Addr::new(172, 40, 0, 3)), "outside 172.16/12: public");
    for a in own_ipv4() {
        assert!(usable_lan(a), "{a}");
    }
    let mut seen = own_ipv4();
    let n = seen.len();
    seen.dedup_by_key(|a| [a.octets()[0], a.octets()[1], a.octets()[2]]);
    assert_eq!(seen.len(), n, "one address per /24");
}

#[test]
fn host_of_reads_ipv4_server_addresses() {
    assert_eq!(host_of("http://10.0.0.20:8096"), Some(Ipv4Addr::new(10, 0, 0, 20)));
    assert_eq!(host_of("https://10.0.0.20/jellyfin"), Some(Ipv4Addr::new(10, 0, 0, 20)));
    assert_eq!(host_of("10.0.0.20"), Some(Ipv4Addr::new(10, 0, 0, 20)));
    assert_eq!(host_of("http://jellyfin.example.test:8096"), None);
}

#[test]
fn standard_options_match_the_brief() {
    let o = Options::standard(Some(Ipv4Addr::new(10, 0, 0, 20)));
    assert_eq!(o.port, DISCOVERY_PORT);
    assert!(o.broadcast);
    assert_eq!(o.broadcast_wait, Duration::from_secs(1));
    assert_eq!(o.gap, Duration::from_millis(2));
    assert_eq!(o.tail, Duration::from_millis(1500));
    assert_eq!(o.extra_hosts, [Ipv4Addr::new(10, 0, 0, 20)]);
}

#[test]
fn report_line_names_servers_and_how_without_secrets() {
    let p = port(7);
    responder([127, 0, 6, 8], p, "srv-r", "Den");
    let r = search(&opts(p, vec![Ipv4Addr::new(127, 0, 6, 1)], vec![]));
    let line = r.line();
    assert!(line.starts_with("discovery:"), "{line}");
    assert!(line.contains("Den http://127.0.6.8:8096 via unicast"), "{line}");
    assert!(line.contains("sent=254"), "{line}");
    assert!(line.contains("own=[127.0.6.1]"), "{line}");
    assert!(line.contains("error=none"), "{line}");
    assert!(r.error.is_none());
}

#[test]
fn a_docker_container_address_is_replaced_by_the_sender() {
    let p = port(8);
    responder_saying([127, 0, 7, 30], p, "dock", "In Docker", "http://172.40.0.3:8096");
    let r = search(&opts(p, vec![Ipv4Addr::new(127, 0, 7, 1)], vec![]));
    assert_eq!(r.found.len(), 1, "{:?}", r.found);
    assert_eq!(r.found[0].server.address, "http://127.0.7.30:8096");
}

#[test]
fn reachable_address_keeps_names_senders_and_own_subnets() {
    let from = Ipv4Addr::new(10, 0, 0, 20);
    let own = [Ipv4Addr::new(10, 0, 0, 7)];
    assert_eq!(reachable_address("http://172.40.0.3:8096", from, &own), "http://10.0.0.20:8096");
    assert_eq!(reachable_address("https://172.40.0.3:8920/jf", from, &own), "https://10.0.0.20:8920/jf");
    assert_eq!(reachable_address("http://10.0.0.20:8096", from, &own), "http://10.0.0.20:8096");
    assert_eq!(reachable_address("http://10.0.0.99:8096", from, &own), "http://10.0.0.99:8096");
    assert_eq!(reachable_address("http://media.example.test:8096", from, &own), "http://media.example.test:8096");
}

#[test]
fn docker_bridges_are_172_17_to_31() {
    for b in 17..=31u8 {
        assert!(docker_bridge(Ipv4Addr::new(172, b, 0, 1)), "172.{b}");
        assert!(!usable_lan(Ipv4Addr::new(172, b, 3, 9)), "172.{b}");
    }
    assert!(!docker_bridge(Ipv4Addr::new(172, 16, 0, 1)));
    assert!(!docker_bridge(Ipv4Addr::new(192, 168, 1, 1)));
    assert!(!docker_bridge(Ipv4Addr::new(10, 0, 0, 1)));
}
