use std::net::{TcpListener, UdpSocket};

use super::world::{WorldError, failed};

pub(crate) const LOCAL_SERVER_HOST: &str = "127.0.0.1";
/// BDS's conventional TCP signaling port, which the container listens on.
pub(crate) const BDS_CONTAINER_HTTP_PORT: u16 = 19132;
pub(crate) const BDS_CONTAINER_UDP_PORT: u16 = 32000;
/// go-nethernet's LAN discovery port, where BDS answers discovery requests.
pub(crate) const LAN_DISCOVERY_PORT: u16 = 7551;
const MAX_PORT: u32 = 65_535;
const AUTOMATIC_ATTEMPTS: usize = 32;

/// The loopback `host:port` of `port`.
pub(crate) fn loopback(port: u16) -> String {
    format!("{LOCAL_SERVER_HOST}:{port}")
}

/// The loopback LAN discovery endpoint; zero uses the default discovery port.
pub(crate) fn lan_address(port: u16) -> String {
    loopback(if port == 0 { LAN_DISCOVERY_PORT } else { port })
}

/// A free loopback UDP port for a Dragonfly server.
pub(crate) fn free_loopback_port() -> Result<u16, WorldError> {
    UdpSocket::bind((LOCAL_SERVER_HOST, 0))
        .and_then(|socket| socket.local_addr())
        .map(|address| address.port())
        .map_err(|error| failed(format!("reserve loopback port: {error}")))
}

/// Reserves the TCP signaling port and the UDP window above it for `players`, or selects one when
/// `port` is zero. The ports are released before the server starts.
pub(crate) fn free_bds_port(players: u32, port: u16) -> Result<u16, WorldError> {
    if !(1..=MAX_PORT - u32::from(BDS_CONTAINER_UDP_PORT) + 1).contains(&players) {
        return Err(failed(format!(
            "player limit {players} does not fit the NetherNet UDP allocation window"
        )));
    }
    if port != 0 {
        if u32::from(port) > MAX_PORT - players + 1 {
            return Err(failed(format!(
                "host port {port} does not fit the NetherNet UDP allocation window for {players} players"
            )));
        }
        return reserve(players, port)?
            .ok_or_else(|| failed(format!("host port {port} is unavailable")));
    }
    for _ in 0..AUTOMATIC_ATTEMPTS {
        if let Some(port) = reserve(players, 0)? {
            return Ok(port);
        }
    }
    Err(failed(format!(
        "no available loopback NetherNet port range for {players} players"
    )))
}

/// Binds the window starting at `requested`; `None` asks an automatic caller to try again.
fn reserve(players: u32, requested: u16) -> Result<Option<u16>, WorldError> {
    let tcp = TcpListener::bind((LOCAL_SERVER_HOST, requested))
        .map_err(|error| failed(format!("reserve signaling port: {error}")))?;
    let port = tcp
        .local_addr()
        .map_err(|error| failed(error.to_string()))?
        .port();
    if u32::from(port) > MAX_PORT - players + 1 {
        return Ok(None);
    }
    let mut udp = Vec::new();
    for offset in 0..players {
        let candidate = u32::from(port) + offset;
        let Ok(candidate) = u16::try_from(candidate) else {
            return Ok(None);
        };
        match UdpSocket::bind((LOCAL_SERVER_HOST, candidate)) {
            Ok(socket) => udp.push(socket),
            Err(error) if requested != 0 => {
                return Err(failed(format!(
                    "reserve UDP transport port {candidate}: {error}"
                )));
            }
            Err(_) => return Ok(None),
        }
    }
    Ok(Some(port))
}

/// Rejects a LAN discovery port that is invalid, overlaps the BDS UDP window or is taken.
pub(crate) fn check_bds_lan_port(
    host_port: u16,
    players: u32,
    lan_port: u16,
) -> Result<(), WorldError> {
    let (lan, host) = (u32::from(lan_port), u32::from(host_port));
    if lan_port == 0 || (host..host + players).contains(&lan) {
        return Err(failed(format!(
            "LAN discovery port {lan_port} is invalid or overlaps the BDS UDP window"
        )));
    }
    match UdpSocket::bind((LOCAL_SERVER_HOST, lan_port)) {
        Ok(_) => Ok(()),
        Err(error) => Err(failed(format!(
            "reserve LAN discovery port {lan_port}: {error}"
        ))),
    }
}

/// `base` alone, or the inclusive range of `count` ports from it.
pub(crate) fn udp_port_range(base: u16, count: u32) -> String {
    if count == 1 {
        base.to_string()
    } else {
        format!("{base}-{}", u32::from(base) + count - 1)
    }
}

/// BDS documents `[ip:]external:internal`; the loopback host keeps Docker's address out of the
/// SDP answer.
pub(crate) fn bds_udp_mapping(external: u16, internal: u16, count: u32) -> String {
    format!(
        "{LOCAL_SERVER_HOST}:{}:{}",
        udp_port_range(external, count),
        udp_port_range(internal, count)
    )
}

#[cfg(test)]
#[path = "ports_tests.rs"]
mod tests;
