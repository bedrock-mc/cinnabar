use super::*;

#[test]
fn automatic_allocation_moves_past_a_reusable_but_udp_blocked_candidate() {
    let players = 3;
    let (first_tcp, _blocked_udp) = reserved_window(players);
    let blocked = first_tcp.local_addr().expect("address").port();
    let mut first_tcp = Some(first_tcp);

    let selected = automatic_bds_port(players, || {
        if let Some(tcp) = first_tcp.take() {
            return Ok(tcp);
        }
        // Model an allocator that reuses the first port whenever its listener is dropped.
        TcpListener::bind((LOCAL_SERVER_HOST, blocked))
            .or_else(|_| TcpListener::bind((LOCAL_SERVER_HOST, 0)))
    })
    .expect("allocation must move to an available TCP and UDP window");
    assert_ne!(selected, blocked);
    bind_released_port("rejected TCP candidate released", || {
        TcpListener::bind((LOCAL_SERVER_HOST, blocked))
    });
    bind_released_port("selected TCP candidate released", || {
        TcpListener::bind((LOCAL_SERVER_HOST, selected))
    });
}

/// Holds an OS-assigned TCP port and its complete UDP window, retrying contested candidates.
fn reserved_window(players: u32) -> (TcpListener, Vec<UdpSocket>) {
    let mut rejected = Vec::new();
    for _ in 0..AUTOMATIC_ATTEMPTS {
        let tcp = TcpListener::bind((LOCAL_SERVER_HOST, 0)).expect("reserve fixture TCP");
        let port = tcp.local_addr().expect("address").port();
        if u32::from(port) <= MAX_PORT - players + 1 {
            let udp: std::io::Result<Vec<_>> = (0..players)
                .map(|offset| {
                    let port = u16::try_from(u32::from(port) + offset).expect("window fits");
                    UdpSocket::bind((LOCAL_SERVER_HOST, port))
                })
                .collect();
            if let Ok(udp) = udp {
                return (tcp, udp);
            }
        }
        rejected.push(tcp);
    }
    panic!("fixture could not reserve a matching TCP/UDP window");
}

/// Rebinds a released port, waiting through competing binds but failing if it stays occupied.
fn bind_released_port<T>(what: &str, mut bind: impl FnMut() -> std::io::Result<T>) -> T {
    let mut socket = None;
    test_time::eventually(what, || match bind() {
        Ok(bound) => {
            socket = Some(bound);
            true
        }
        Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => false,
        Err(error) => panic!("{what}: {error}"),
    });
    socket.expect(what)
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
        bind_released_port("rejected TCP candidate released", || {
            TcpListener::bind((LOCAL_SERVER_HOST, port))
        });
    }
}

#[test]
fn automatic_windows_reserve_both_protocols_and_release_every_port() {
    let players = 3;
    let port = free_bds_port(players, 0).expect("window");
    let _tcp = bind_released_port("TCP released", || {
        TcpListener::bind((LOCAL_SERVER_HOST, port))
    });
    let _udp: Vec<UdpSocket> = (0..players)
        .map(|offset| {
            let port = u16::try_from(u32::from(port) + offset).expect("port");
            bind_released_port("UDP released", || {
                UdpSocket::bind((LOCAL_SERVER_HOST, port))
            })
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
    test_time::eventually("fixed window available after automatic allocation", || {
        match free_bds_port(players, port) {
            Ok(selected) => {
                assert_eq!(selected, port);
                true
            }
            Err(_) => false,
        }
    });
    {
        let _taken =
            bind_released_port("take TCP", || TcpListener::bind((LOCAL_SERVER_HOST, port)));
        assert!(
            free_bds_port(players, port).is_err(),
            "an occupied fixed TCP port must fail instead of moving"
        );
    }
    {
        let (port, taken) = blocked_udp_window(players);
        let last = taken.local_addr().expect("address").port();
        test_time::eventually("UDP window rejected at its occupied final port", || {
            free_bds_port(players, port)
                .expect_err("occupied UDP window")
                .to_string()
                .contains(&format!("UDP transport port {last}:"))
        });
        let _tcp = bind_released_port("failed UDP reservation released its TCP port", || {
            TcpListener::bind((LOCAL_SERVER_HOST, port))
        });
        let _udp = bind_released_port("failed UDP reservation released earlier UDP ports", || {
            UdpSocket::bind((LOCAL_SERVER_HOST, port))
        });
    }
    assert!(
        free_bds_port(players, 65_535).is_err(),
        "a wrapping fixed window must fail"
    );
}

/// Holds a window's last UDP port after checking its TCP port and earlier UDP ports are available.
fn blocked_udp_window(players: u32) -> (u16, UdpSocket) {
    let (tcp, mut udp) = reserved_window(players);
    let port = tcp.local_addr().expect("address").port();
    (port, udp.pop().expect("nonempty UDP window"))
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
    test_time::eventually("released discovery port", || {
        check_bds_lan_port(19_132, 8, port).is_ok()
    });
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
