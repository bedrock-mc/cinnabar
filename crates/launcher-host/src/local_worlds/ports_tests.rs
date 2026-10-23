use super::*;

#[test]
fn automatic_allocation_moves_past_a_reusable_but_udp_blocked_candidate() {
    let players = 3;
    let blocked = free_bds_port(players, 0).expect("first window");
    let first_tcp = TcpListener::bind((LOCAL_SERVER_HOST, blocked)).expect("hold first TCP");
    let _blocked_udp: Vec<_> = (0..players)
        .map(|offset| {
            let port = u16::try_from(u32::from(blocked) + offset).expect("port");
            UdpSocket::bind((LOCAL_SERVER_HOST, port)).expect("occupy first UDP window")
        })
        .collect();
    let available = free_bds_port(players, 0).expect("second window");
    drop(first_tcp);

    let selected = automatic_bds_port(players, || {
        // Model an allocator that reuses the first port whenever its listener is dropped.
        TcpListener::bind((LOCAL_SERVER_HOST, blocked))
            .or_else(|_| TcpListener::bind((LOCAL_SERVER_HOST, available)))
    })
    .expect("the second candidate has an available TCP and UDP window");
    assert_eq!(selected, available);
    TcpListener::bind((LOCAL_SERVER_HOST, blocked)).expect("rejected TCP candidate released");
    TcpListener::bind((LOCAL_SERVER_HOST, available)).expect("selected TCP candidate released");
}

#[test]
fn exhausted_automatic_allocation_releases_rejected_signaling_ports() {
    let mut occupied_udp = Vec::new();
    let mut candidates = Vec::new();
    let error = automatic_bds_port(1, || {
        let mut rejected_udp = Vec::new();
        for _ in 0..AUTOMATIC_ATTEMPTS {
            let udp = UdpSocket::bind((LOCAL_SERVER_HOST, 0))?;
            let port = udp.local_addr()?.port();
            if let Ok(tcp) = TcpListener::bind((LOCAL_SERVER_HOST, port)) {
                occupied_udp.push(udp);
                candidates.push(port);
                return Ok(tcp);
            }
            // Keep failed fixture candidates occupied until both protocols can be held.
            rejected_udp.push(udp);
        }
        Err(std::io::Error::other(
            "fixture could not reserve a matching TCP/UDP pair",
        ))
    })
    .expect_err("every candidate has an occupied UDP port");
    assert!(error.to_string().contains("UDP transport port"), "{error}");
    assert_eq!(candidates.len(), AUTOMATIC_ATTEMPTS);
    for port in candidates {
        TcpListener::bind((LOCAL_SERVER_HOST, port)).expect("rejected TCP candidate released");
    }
}

#[test]
fn automatic_windows_reserve_both_protocols_and_release_every_port() {
    let players = 3;
    let port = free_bds_port(players, 0).expect("window");
    let _tcp = TcpListener::bind((LOCAL_SERVER_HOST, port)).expect("TCP released");
    let _udp: Vec<UdpSocket> = (0..players)
        .map(|offset| {
            let port = u16::try_from(u32::from(port) + offset).expect("port");
            let mut socket = None;
            test_time::eventually("released UDP transport port", || {
                match UdpSocket::bind((LOCAL_SERVER_HOST, port)) {
                    Ok(bound) => {
                        socket = Some(bound);
                        true
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => false,
                    Err(error) => panic!("UDP released: {error}"),
                }
            });
            socket.expect("UDP released")
        })
        .collect();
    for invalid in [0, 65_535] {
        assert!(
            free_bds_port(invalid, 0).is_err(),
            "{invalid} players must not get an empty or wrapping window"
        );
    }
}

#[test]
fn a_fixed_port_reserves_its_exact_window_or_fails() {
    let players = 3;
    let port = free_bds_port(players, 0).expect("window");
    assert_eq!(free_bds_port(players, port).expect("fixed window"), port);
    {
        let _taken = TcpListener::bind((LOCAL_SERVER_HOST, port)).expect("take TCP");
        assert!(
            free_bds_port(players, port).is_err(),
            "an occupied fixed TCP port must fail instead of moving"
        );
    }
    {
        let (port, taken) = blocked_udp_window(players);
        let last = taken.local_addr().expect("address").port();
        let error = free_bds_port(players, port).expect_err("occupied UDP window");
        assert!(
            error
                .to_string()
                .contains(&format!("UDP transport port {last}:")),
            "{error}"
        );
        let _tcp = TcpListener::bind((LOCAL_SERVER_HOST, port))
            .expect("a failed UDP reservation leaked its TCP port");
        let _udp = UdpSocket::bind((LOCAL_SERVER_HOST, port))
            .expect("a failed UDP reservation leaked an earlier UDP port");
    }
    assert!(
        free_bds_port(players, 65_535).is_err(),
        "a wrapping fixed window must fail"
    );
}

/// Holds a window's last UDP port after checking its TCP port and earlier UDP ports are available.
fn blocked_udp_window(players: u32) -> (u16, UdpSocket) {
    let mut rejected = Vec::new();
    for _ in 0..AUTOMATIC_ATTEMPTS {
        let first = UdpSocket::bind((LOCAL_SERVER_HOST, 0)).expect("reserve fixture UDP");
        let port = first.local_addr().expect("address").port();
        let Some(last) = port.checked_add(u16::try_from(players - 1).expect("window fits")) else {
            rejected.push(first);
            continue;
        };
        if let Ok(_tcp) = TcpListener::bind((LOCAL_SERVER_HOST, port)) {
            let later: std::io::Result<Vec<_>> = (port..last)
                .map(|candidate| UdpSocket::bind((LOCAL_SERVER_HOST, candidate + 1)))
                .collect();
            if let Ok(mut later) = later {
                return (port, later.pop().unwrap_or(first));
            }
        }
        // A forward window cannot overlap an earlier rejected base when the OS
        // assigns consecutive candidates.
        rejected.push(first);
    }
    panic!("fixture could not reserve a window with only its last UDP port blocked");
}

#[test]
fn lan_discovery_ports_must_be_free_and_outside_the_window() {
    for invalid in [0, 19_132, 19_132 + 7] {
        assert!(check_bds_lan_port(19_132, 8, invalid).is_err(), "{invalid}");
    }
    let taken = UdpSocket::bind((LOCAL_SERVER_HOST, 0)).expect("take a port");
    let port = taken.local_addr().expect("address").port();
    assert!(
        check_bds_lan_port(19_132, 8, port).is_err(),
        "an occupied discovery port must fail before launch"
    );
    drop(taken);
    check_bds_lan_port(19_132, 8, port).expect("released discovery port");
    assert_eq!(lan_address(0), format!("127.0.0.1:{LAN_DISCOVERY_PORT}"));
    assert_eq!(
        lan_address(LAN_DISCOVERY_PORT + 1),
        format!("127.0.0.1:{}", LAN_DISCOVERY_PORT + 1)
    );
}

#[test]
fn udp_windows_name_every_port() {
    assert_eq!(udp_port_range(5000, 1), "5000");
    assert_eq!(udp_port_range(5000, 8), "5000-5007");
    assert_eq!(
        bds_udp_mapping(5000, 32000, 8),
        "127.0.0.1:5000-5007:32000-32007"
    );
}
