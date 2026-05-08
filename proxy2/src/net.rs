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
    packet::Direction,
    translate::{Emit, SessionTranslator, Translator},
};

const MAX_DATAGRAM: usize = 65_535;
const LOOP_SLEEP: Duration = Duration::from_millis(1);

#[derive(Debug)]
struct Session {
    client: SocketAddr,
    upstream: UdpSocket,
    ee_crypto: EeCrypto,
    translator: SessionTranslator,
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
                let session = ensure_session(config, translator_template, sessions, client)?;
                session.last_seen = Instant::now();
                let plain = match session.ee_crypto.preprocess_client_packet(bytes) {
                    Ok(ClientPacket::Plain(plain)) => plain,
                    Ok(ClientPacket::ServerResponse(response)) => {
                        listen.send_to(&response, client).with_context(|| {
                            format!("sending EE crypto response to client {client}")
                        })?;
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
                    Emit::Packets(outbounds)
                    | Emit::PacketsPreShifted(outbounds)
                    | Emit::VerifiedPackets { packets: outbounds, .. }
                    | Emit::VerifiedPacketsPreShifted { packets: outbounds, .. } => {
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
                    Emit::Consumed | Emit::Drop => {}
                }
            }
            Err(err) if err.kind() == ErrorKind::WouldBlock => return Ok(()),
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
        let Some(session) = sessions.get_mut(&client) else {
            continue;
        };
        loop {
            match session.upstream.recv_from(recv_buf) {
                Ok((len, server)) => {
                    let bytes = &recv_buf[..len];
                    session.last_seen = Instant::now();
                    match session
                        .translator
                        .translate(Direction::ServerToClient, bytes)
                    {
                        Emit::Packet(outbound) => {
                            let outbound = session
                                .ee_crypto
                                .encrypt_server_packet_if_needed(&outbound)
                                .context("encrypting server packet for EE client")?;
                            listen.send_to(&outbound, session.client).with_context(|| {
                                format!("sending server datagram to client {}", session.client)
                            })?;
                        }
                        Emit::Packets(outbounds)
                        | Emit::PacketsPreShifted(outbounds)
                        | Emit::VerifiedPackets { packets: outbounds, .. }
                        | Emit::VerifiedPacketsPreShifted { packets: outbounds, .. } => {
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
                        Emit::Consumed => {}
                        Emit::Drop => {
                            tracing::warn!(%server, client = %session.client, "server datagram quarantined");
                        }
                    }
                }
                Err(err) if err.kind() == ErrorKind::WouldBlock => break,
                Err(err) => return Err(err).context("receiving from upstream server socket"),
            }
        }
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
        tracing::info!(
            %client,
            server = %config.server,
            upstream = %upstream.local_addr().unwrap_or_else(|_| "0.0.0.0:0".parse().expect("valid fallback address")),
            "created proxy2 session"
        );
        sessions.insert(
            client,
            Session {
                client,
                upstream,
                ee_crypto: EeCrypto::new().context("initializing EE crypto for proxy2 session")?,
                translator: translator_template.new_session(),
                last_seen: Instant::now(),
            },
        );
    }
    Ok(sessions.get_mut(&client).expect("session inserted"))
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
