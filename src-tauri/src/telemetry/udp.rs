//! UDP link to the PlayStation: heartbeats, auto-discovery and receive loop.
//!
//! GT7 streams ~60 packets/s to whoever sent the last heartbeat to port 33739,
//! and stops after a few seconds without one.

use serde::Serialize;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use super::crypto::{self, Variant};
use super::packet::Packet;

pub const SEND_PORT: u16 = 33739;
pub const RECV_PORT: u16 = 33740;

const HEARTBEAT_EVERY: Duration = Duration::from_millis(1000);
const SWEEP_EVERY: Duration = Duration::from_secs(2);
const LIVE_TIMEOUT: Duration = Duration::from_secs(2);
const VARIANT_CYCLE: Duration = Duration::from_secs(6);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnState {
    Idle,
    Searching,
    Connecting,
    Live,
    Lost,
    Error,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnStatus {
    pub state: ConnState,
    pub ip: Option<String>,
    pub variant: Option<Variant>,
    pub hz: f32,
    pub error: Option<String>,
}

impl ConnStatus {
    pub fn idle() -> Self {
        ConnStatus { state: ConnState::Idle, ip: None, variant: None, hz: 0.0, error: None }
    }
}

pub trait Handler: Send + 'static {
    fn packet(&mut self, p: Packet);
    fn status(&mut self, s: &ConnStatus);
}

pub struct Link {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl Link {
    /// Start the link. With `ip = None` the PlayStation is discovered on the LAN.
    pub fn start<H: Handler>(ip: Option<IpAddr>, handler: H) -> Link {
        let stop = Arc::new(AtomicBool::new(false));
        let s = stop.clone();
        let handle = std::thread::Builder::new()
            .name("gt7-link".into())
            .spawn(move || run(ip, s, handler))
            .expect("spawn link thread");
        Link { stop, handle: Some(handle) }
    }
}

impl Drop for Link {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

fn next_variant(v: Variant) -> Variant {
    match v {
        Variant::Tilde => Variant::B,
        Variant::B => Variant::A,
        Variant::A => Variant::Tilde,
    }
}

/// Heartbeat every host on each private /24 we're attached to, plus broadcast.
fn sweep(sock: &UdpSocket, hb: &[u8]) {
    if let Ok(ifs) = local_ip_address::list_afinet_netifas() {
        for (_, ip) in ifs {
            let IpAddr::V4(v4) = ip else { continue };
            if !v4.is_private() || v4.is_loopback() {
                continue;
            }
            let o = v4.octets();
            for host in 1..=255u8 {
                if host != o[3] {
                    let _ = sock.send_to(hb, (Ipv4Addr::new(o[0], o[1], o[2], host), SEND_PORT));
                }
            }
        }
    }
    let _ = sock.send_to(hb, (Ipv4Addr::BROADCAST, SEND_PORT));
}

fn run<H: Handler>(fixed_ip: Option<IpAddr>, stop: Arc<AtomicBool>, mut h: H) {
    let sock = match UdpSocket::bind(SocketAddr::from(([0, 0, 0, 0], RECV_PORT))) {
        Ok(s) => s,
        Err(e) => {
            h.status(&ConnStatus {
                state: ConnState::Error,
                error: Some(format!("Could not open UDP port {RECV_PORT}: {e}. Is another telemetry app running?")),
                ..ConnStatus::idle()
            });
            return;
        }
    };
    let _ = sock.set_broadcast(true);
    let _ = sock.set_read_timeout(Some(Duration::from_millis(50)));

    let mut target = fixed_ip;
    let mut variant = Variant::Tilde;
    let mut variant_since = Instant::now();
    let mut last_hb: Option<Instant> = None;
    let mut last_sweep: Option<Instant> = None;
    let mut last_rx: Option<Instant> = None;
    let mut rx_window = 0u32;
    let mut window_start = Instant::now();
    let mut hz = 0.0f32;
    let mut last_status: Option<(ConnState, Instant)> = None;
    let mut buf = [0u8; 2048];

    while !stop.load(Ordering::Relaxed) {
        let now = Instant::now();
        let live = last_rx.is_some_and(|t| now - t < LIVE_TIMEOUT);

        if last_hb.map_or(true, |t| now - t >= HEARTBEAT_EVERY) {
            if let Some(ip) = target {
                let _ = sock.send_to(variant.heartbeat(), (ip, SEND_PORT));
            }
            if !live && fixed_ip.is_none() && last_sweep.map_or(true, |t| now - t >= SWEEP_EVERY) {
                sweep(&sock, variant.heartbeat());
                last_sweep = Some(now);
            }
            last_hb = Some(now);
        }
        // Older game versions only answer the original "A" heartbeat.
        if !live && now - variant_since > VARIANT_CYCLE {
            variant = next_variant(variant);
            variant_since = now;
            last_hb = None;
        }

        if let Ok((n, src)) = sock.recv_from(&mut buf) {
            if let Some(p) = crypto::decrypt(&buf[..n]).and_then(|(plain, v)| {
                variant = v;
                Packet::parse(&plain)
            }) {
                variant_since = now;
                if fixed_ip.is_none() {
                    target = Some(src.ip());
                }
                last_rx = Some(now);
                rx_window += 1;
                h.packet(p);
            }
        }

        let elapsed = now - window_start;
        if elapsed >= Duration::from_secs(1) {
            hz = rx_window as f32 / elapsed.as_secs_f32();
            rx_window = 0;
            window_start = now;
        }
        let live = last_rx.is_some_and(|t| now - t < LIVE_TIMEOUT);
        let state = match (live, last_rx.is_some(), fixed_ip.is_some()) {
            (true, _, _) => ConnState::Live,
            (false, true, _) => ConnState::Lost,
            (false, false, true) => ConnState::Connecting,
            (false, false, false) => ConnState::Searching,
        };
        let due = last_status.map_or(true, |(s, t)| s != state || now - t >= Duration::from_secs(1));
        if due {
            h.status(&ConnStatus {
                state,
                ip: target.map(|ip| ip.to_string()),
                variant: live.then_some(variant),
                hz: if live { hz } else { 0.0 },
                error: None,
            });
            last_status = Some((state, now));
        }
    }
    h.status(&ConnStatus::idle());
}
