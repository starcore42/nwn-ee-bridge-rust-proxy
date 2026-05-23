use std::{
    collections::HashMap,
    io::ErrorKind,
    net::{SocketAddr, UdpSocket},
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};

use crate::{
    config::Config,
    ee_crypto::{ClientPacket, EeCrypto},
    nwsync,
    packet::{
        Direction,
        m::{LEGACY_GAMEPLAY_PAYLOAD_OFFSET, MFrameView},
    },
    translate::{Emit, SessionTranslator, Translator},
};

const MAX_DATAGRAM: usize = 65_535;
const LOOP_SLEEP: Duration = Duration::from_millis(1);
const EE_CRYPTO_DEFAULT_RESPONSE_DEFER: Duration = Duration::from_millis(100);
const EE_CRYPTO_BNK2_RESPONSE_DEFER: Duration = Duration::from_millis(500);
const MAX_SERVER_DATAGRAMS_PER_TICK_PER_SESSION: usize = 16;

#[derive(Debug)]
struct Session {
    client: SocketAddr,
    upstream: UdpSocket,
    ee_crypto: EeCrypto,
    translator: SessionTranslator,
    pending_ee_crypto_responses: Vec<(Instant, Vec<u8>)>,
    last_seen: Instant,
}

pub fn run(config: Config, nwsync_runtime: Option<nwsync::Runtime>) -> Result<()> {
    let listen = UdpSocket::bind(config.listen)
        .with_context(|| format!("binding listen socket {}", config.listen))?;
    listen
        .set_nonblocking(true)
        .context("setting listen socket nonblocking")?;

    let _nwsync_http = nwsync::start_http_server_if_needed(&config, nwsync_runtime.as_ref())?;
    let translator_template = Translator::new(&config, nwsync_runtime)?;
    let mut sessions: HashMap<SocketAddr, Session> = HashMap::new();
    let mut recv_buf = vec![0_u8; MAX_DATAGRAM];

    tracing::info!(
        listen = %config.listen,
        server = %config.server,
        "proxy2 UDP bridge ready"
    );

    loop {
        drain_pending_ee_crypto_responses(&listen, &mut sessions)?;
        drain_pending_proxy_packets_for_all_sessions(&config, &listen, &mut sessions)?;
        drain_client_socket(
            &config,
            &translator_template,
            &listen,
            &mut sessions,
            &mut recv_buf,
        )?;
        drain_server_sockets(&listen, &mut sessions, &mut recv_buf)?;
        expire_sessions(&config, &mut sessions);
        thread::sleep(LOOP_SLEEP);
    }
}

fn is_udp_connection_reset(err: &std::io::Error) -> bool {
    matches!(
        err.kind(),
        ErrorKind::ConnectionReset | ErrorKind::ConnectionAborted
    ) || err.raw_os_error() == Some(10054)
}

fn drain_client_socket(
    config: &Config,
    translator_template: &Translator,
    listen: &UdpSocket,
    sessions: &mut HashMap<SocketAddr, Session>,
    recv_buf: &mut [u8],
) -> Result<()> {
    loop {
        match listen.recv_from(recv_buf) {
            Ok((len, client)) => {
                if !config.allow_remote_clients && !client.ip().is_loopback() {
                    tracing::warn!(%client, "dropping non-loopback client; use --allow-remote-clients to permit it");
                    continue;
                }
                let bytes = &recv_buf[..len];
                let mut retire_session_after_emit: Option<&'static str> = None;
                let session = ensure_session(config, translator_template, sessions, client)?;
                session.last_seen = Instant::now();
                let plain = match session.ee_crypto.preprocess_client_packet(bytes) {
                    Ok(ClientPacket::Plain(plain)) => plain,
                    Ok(ClientPacket::ServerResponse(response)) => {
                        // EE 8193.37 `StartConnectToSession` sends BNK1 before
                        // completing the post-BNK1 connection-state writes
                        // (`m_kx_stage = 1`, player/CD-key strings, timeout).
                        // On loopback, an immediate BNK2 can be delivered into
                        // `HandleBNK2Message` while the native StartConnect
                        // frame is still unwinding. Queue BNK2 for a bounded
                        // local transport delay so the packet bytes remain
                        // exact while delivery matches the non-reentrant
                        // ordering a real remote server naturally provides.
                        let defer = ee_crypto_response_defer(&response);
                        let due = Instant::now() + defer;
                        tracing::info!(
                            %client,
                            len = response.len(),
                            defer_ms = defer.as_millis(),
                            tag = %String::from_utf8_lossy(response.get(..4).unwrap_or(&[])),
                            "queued EE crypto response for deferred delivery"
                        );
                        session.pending_ee_crypto_responses.push((due, response));
                        continue;
                    }
                    Ok(ClientPacket::Consumed) => continue,
                    Err(err) => {
                        tracing::warn!(%client, error = %err, "dropping client packet during EE crypto preprocess");
                        continue;
                    }
                };
                match session
                    .translator
                    .translate(Direction::ClientToServer, &plain)
                {
                    Emit::Packet(outbound) => {
                        session
                            .upstream
                            .send_to(&outbound, config.server)
                            .with_context(|| {
                                format!("sending client datagram to server {}", config.server)
                            })?;
                    }
                    Emit::PacketRetireSession { packet, reason } => {
                        session
                            .upstream
                            .send_to(&packet, config.server)
                            .with_context(|| {
                                format!("sending client datagram to server {}", config.server)
                            })?;
                        retire_session_after_emit = Some(reason);
                    }
                    Emit::Packets(outbounds)
                    | Emit::PacketsPreShifted(outbounds)
                    | Emit::VerifiedPackets {
                        packets: outbounds, ..
                    }
                    | Emit::VerifiedPacketsPreShifted {
                        packets: outbounds, ..
                    }
                    | Emit::VerifiedProofPackets {
                        packets: outbounds, ..
                    }
                    | Emit::VerifiedProofPacketsPreShifted {
                        packets: outbounds, ..
                    } => {
                        for outbound in outbounds {
                            session
                                .upstream
                                .send_to(&outbound, config.server)
                                .with_context(|| {
                                    format!("sending client datagram to server {}", config.server)
                                })?;
                        }
                    }
                    Emit::MixedVerifiedPackets(outbounds) => {
                        for (_, outbound) in outbounds {
                            session
                                .upstream
                                .send_to(&outbound, config.server)
                                .with_context(|| {
                                    format!("sending client datagram to server {}", config.server)
                                })?;
                        }
                    }
                    Emit::MixedVerifiedProofPackets(outbounds)
                    | Emit::MixedVerifiedProofPacketsPreShifted(outbounds) => {
                        for (_, outbound) in outbounds {
                            session
                                .upstream
                                .send_to(&outbound, config.server)
                                .with_context(|| {
                                    format!("sending client datagram to server {}", config.server)
                                })?;
                        }
                    }
                    Emit::Consumed => {}
                    Emit::ConsumedRetireSession { reason } => {
                        retire_session_after_emit = Some(reason);
                    }
                    Emit::Drop => {}
                }
                if let Some(reason) = retire_session_after_emit {
                    tracing::info!(
                        %client,
                        reason,
                        "retiring proxy2 session after client disconnect control packet"
                    );
                    sessions.remove(&client);
                } else {
                    send_pending_client_to_server_packets(session, config.server)?;
                    send_pending_server_to_client_packets(listen, session)?;
                }
            }
            Err(err) if err.kind() == ErrorKind::WouldBlock => return Ok(()),
            Err(err) if is_udp_connection_reset(&err) => {
                let retired_sessions = sessions.len();
                sessions.clear();
                tracing::warn!(
                    error = %err,
                    retired_sessions,
                    "UDP client-socket connection reset observed; retired active proxy2 sessions instead of replaying server traffic to a closed EE client"
                );
                return Ok(());
            }
            Err(err) => return Err(err).context("receiving from client socket"),
        }
    }
}

fn drain_server_sockets(
    listen: &UdpSocket,
    sessions: &mut HashMap<SocketAddr, Session>,
    recv_buf: &mut [u8],
) -> Result<()> {
    let clients: Vec<SocketAddr> = sessions.keys().copied().collect();
    for client in clients {
        let mut retire_session_after_emit: Option<&'static str> = None;
        let Some(session) = sessions.get_mut(&client) else {
            continue;
        };
        let mut processed_server_datagrams = 0usize;
        loop {
            if processed_server_datagrams >= MAX_SERVER_DATAGRAMS_PER_TICK_PER_SESSION {
                break;
            }
            match session.upstream.recv_from(recv_buf) {
                Ok((len, server)) => {
                    processed_server_datagrams = processed_server_datagrams.saturating_add(1);
                    let bytes = &recv_buf[..len];
                    session.last_seen = Instant::now();
                    let emit = session
                        .translator
                        .translate(Direction::ServerToClient, bytes);
                    send_pending_client_to_server_packets(session, server)?;
                    send_pending_server_to_client_packets(listen, session)?;
                    match emit {
                        Emit::Packet(outbound) => {
                            let outbound = session
                                .ee_crypto
                                .encrypt_server_packet_if_needed(&outbound)
                                .context("encrypting server packet for EE client")?;
                            listen.send_to(&outbound, session.client).with_context(|| {
                                format!("sending server datagram to client {}", session.client)
                            })?;
                        }
                        Emit::PacketRetireSession { packet, reason } => {
                            let outbound = session
                                .ee_crypto
                                .encrypt_server_packet_if_needed(&packet)
                                .context("encrypting server retire packet for EE client")?;
                            listen.send_to(&outbound, session.client).with_context(|| {
                                format!(
                                    "sending server retire datagram to client {}",
                                    session.client
                                )
                            })?;
                            retire_session_after_emit = Some(reason);
                            break;
                        }
                        Emit::Packets(outbounds)
                        | Emit::PacketsPreShifted(outbounds)
                        | Emit::VerifiedPackets {
                            packets: outbounds, ..
                        }
                        | Emit::VerifiedPacketsPreShifted {
                            packets: outbounds, ..
                        }
                        | Emit::VerifiedProofPackets {
                            packets: outbounds, ..
                        }
                        | Emit::VerifiedProofPacketsPreShifted {
                            packets: outbounds, ..
                        } => {
                            for outbound in outbounds {
                                let outbound = session
                                    .ee_crypto
                                    .encrypt_server_packet_if_needed(&outbound)
                                    .context("encrypting server packet for EE client")?;
                                listen.send_to(&outbound, session.client).with_context(|| {
                                    format!("sending server datagram to client {}", session.client)
                                })?;
                            }
                        }
                        Emit::MixedVerifiedPackets(outbounds) => {
                            for (_, outbound) in outbounds {
                                let outbound = session
                                    .ee_crypto
                                    .encrypt_server_packet_if_needed(&outbound)
                                    .context("encrypting server packet for EE client")?;
                                listen.send_to(&outbound, session.client).with_context(|| {
                                    format!("sending server datagram to client {}", session.client)
                                })?;
                            }
                        }
                        Emit::MixedVerifiedProofPackets(outbounds)
                        | Emit::MixedVerifiedProofPacketsPreShifted(outbounds) => {
                            for (_, outbound) in outbounds {
                                let outbound = session
                                    .ee_crypto
                                    .encrypt_server_packet_if_needed(&outbound)
                                    .context("encrypting server packet for EE client")?;
                                listen.send_to(&outbound, session.client).with_context(|| {
                                    format!("sending server datagram to client {}", session.client)
                                })?;
                            }
                        }
                        Emit::Consumed => {}
                        Emit::ConsumedRetireSession { reason } => {
                            retire_session_after_emit = Some(reason);
                            break;
                        }
                        Emit::Drop => {
                            tracing::warn!(%server, client = %session.client, "server datagram quarantined");
                        }
                    }
                }
                Err(err) if err.kind() == ErrorKind::WouldBlock => break,
                Err(err) if is_udp_connection_reset(&err) => {
                    tracing::warn!(
                        %client,
                        error = %err,
                        "ignoring UDP upstream connection reset for session"
                    );
                    break;
                }
                Err(err) => return Err(err).context("receiving from upstream server socket"),
            }
        }
        if let Some(reason) = retire_session_after_emit {
            tracing::info!(
                %client,
                reason,
                "retiring proxy2 session after server disconnect control packet"
            );
            sessions.remove(&client);
        }
    }
    Ok(())
}

fn send_pending_server_to_client_packets(listen: &UdpSocket, session: &mut Session) -> Result<()> {
    for outbound in session.translator.take_pending_server_to_client_packets() {
        let plain_len = outbound.len();
        let plain_prefix = crate::packet::hex_prefix(&outbound, 32);
        let outbound = session
            .ee_crypto
            .encrypt_server_packet_if_needed(&outbound)
            .context("encrypting pending server packet for EE client")?;
        tracing::info!(
            client = %session.client,
            plain_len,
            encrypted_len = outbound.len(),
            plain_prefix = %plain_prefix,
            encrypted_prefix = %crate::packet::hex_prefix(&outbound, 24),
            "sending pending server-to-client proxy-owned datagram"
        );
        listen.send_to(&outbound, session.client).with_context(|| {
            format!(
                "sending pending server datagram to client {}",
                session.client
            )
        })?;
    }
    Ok(())
}

fn send_pending_client_to_server_packets(session: &mut Session, server: SocketAddr) -> Result<()> {
    for outbound in session.translator.take_pending_client_to_server_packets() {
        log_proxy_generated_client_packet(session.client, server, &outbound);
        session
            .upstream
            .send_to(&outbound, server)
            .with_context(|| format!("sending proxy-owned client datagram to server {server}"))?;
    }
    Ok(())
}

fn drain_pending_proxy_packets_for_all_sessions(
    config: &Config,
    listen: &UdpSocket,
    sessions: &mut HashMap<SocketAddr, Session>,
) -> Result<()> {
    for session in sessions.values_mut() {
        send_pending_client_to_server_packets(session, config.server)?;
        send_pending_server_to_client_packets(listen, session)?;
    }
    Ok(())
}

fn ensure_session<'a>(
    config: &Config,
    translator_template: &Translator,
    sessions: &'a mut HashMap<SocketAddr, Session>,
    client: SocketAddr,
) -> Result<&'a mut Session> {
    if !sessions.contains_key(&client) {
        let upstream =
            UdpSocket::bind("0.0.0.0:0").context("binding per-client upstream UDP socket")?;
        upstream
            .set_nonblocking(true)
            .context("setting upstream socket nonblocking")?;
        let upstream_local_addr = upstream
            .local_addr()
            .context("reading per-client upstream UDP socket address")?;
        let legacy_udp_port = upstream_local_addr.port();
        tracing::info!(
            %client,
            server = %config.server,
            upstream = %upstream_local_addr,
            legacy_udp_port,
            "created proxy2 session"
        );
        sessions.insert(
            client,
            Session {
                client,
                upstream,
                ee_crypto: EeCrypto::new().context("initializing EE crypto for proxy2 session")?,
                translator: translator_template.new_session(legacy_udp_port),
                pending_ee_crypto_responses: Vec::new(),
                last_seen: Instant::now(),
            },
        );
    }
    Ok(sessions.get_mut(&client).expect("session inserted"))
}

fn drain_pending_ee_crypto_responses(
    listen: &UdpSocket,
    sessions: &mut HashMap<SocketAddr, Session>,
) -> Result<()> {
    let now = Instant::now();
    for session in sessions.values_mut() {
        let mut index = 0;
        while index < session.pending_ee_crypto_responses.len() {
            if session.pending_ee_crypto_responses[index].0 > now {
                index += 1;
                continue;
            }
            let (_, response) = session.pending_ee_crypto_responses.remove(index);
            tracing::info!(
                client = %session.client,
                len = response.len(),
                tag = %String::from_utf8_lossy(response.get(..4).unwrap_or(&[])),
                "sending deferred EE crypto response"
            );
            listen.send_to(&response, session.client).with_context(|| {
                format!(
                    "sending deferred EE crypto response to client {}",
                    session.client
                )
            })?;
        }
    }
    Ok(())
}

fn ee_crypto_response_defer(response: &[u8]) -> Duration {
    if response.get(..4) == Some(b"BNK2") {
        EE_CRYPTO_BNK2_RESPONSE_DEFER
    } else {
        EE_CRYPTO_DEFAULT_RESPONSE_DEFER
    }
}

fn expire_sessions(config: &Config, sessions: &mut HashMap<SocketAddr, Session>) {
    let now = Instant::now();
    let timeout = config.session_timeout();
    sessions.retain(|client, session| {
        let alive = now.duration_since(session.last_seen) <= timeout;
        if !alive {
            tracing::info!(%client, "expired proxy2 session");
        }
        alive
    });
}

fn log_proxy_generated_client_packet(client: SocketAddr, server: SocketAddr, bytes: &[u8]) {
    let parsed = MFrameView::parse(bytes);
    let high = parsed.as_ref().and_then(|view| {
        let end = LEGACY_GAMEPLAY_PAYLOAD_OFFSET.saturating_add(view.payload_length);
        bytes
            .get(LEGACY_GAMEPLAY_PAYLOAD_OFFSET..end)
            .and_then(crate::packet::m::HighLevel::parse)
    });
    tracing::info!(
        %client,
        %server,
        len = bytes.len(),
        sequence = parsed.as_ref().map(|view| view.sequence),
        ack_sequence = parsed.as_ref().map(|view| view.ack_sequence),
        payload_length = parsed.as_ref().map(|view| view.payload_length),
        crc_valid = parsed.as_ref().map(|view| view.crc_valid),
        high_major = high.as_ref().map(|high| high.major),
        high_minor = high.as_ref().map(|high| high.minor),
        high_name = high.as_ref().map(|high| high.name()),
        "sending proxy-generated client-to-server packet"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bnk2_response_uses_conservative_loopback_defer() {
        assert_eq!(
            ee_crypto_response_defer(b"BNK2payload"),
            EE_CRYPTO_BNK2_RESPONSE_DEFER
        );
    }

    #[test]
    fn non_bnk2_crypto_response_keeps_default_defer() {
        assert_eq!(
            ee_crypto_response_defer(b"BNK4\x01\x02\x03\x04"),
            EE_CRYPTO_DEFAULT_RESPONSE_DEFER
        );
        assert_eq!(
            ee_crypto_response_defer(b""),
            EE_CRYPTO_DEFAULT_RESPONSE_DEFER
        );
    }
}
