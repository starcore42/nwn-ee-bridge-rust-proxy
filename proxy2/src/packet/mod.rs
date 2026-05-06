pub mod bn;
pub mod m;

use std::fmt;

use bn::BnPacket;
use m::MFrame;

#[derive(Debug, Clone)]
pub enum Packet<'a> {
    Bn(BnPacket<'a>),
    M(MFrame<'a>),
    UnknownTopLevel(&'a [u8]),
}

impl<'a> Packet<'a> {
    pub fn classify(bytes: &'a [u8]) -> Self {
        if bytes.starts_with(b"BN") {
            return Packet::Bn(BnPacket::parse(bytes));
        }
        if bytes.first() == Some(&b'M') {
            return Packet::M(MFrame::parse(bytes));
        }
        Packet::UnknownTopLevel(bytes)
    }

    pub fn family(&self) -> &'static str {
        match self {
            Packet::Bn(_) => "BN",
            Packet::M(_) => "M",
            Packet::UnknownTopLevel(_) => "top-level",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    ClientToServer,
    ServerToClient,
    ServerToClientSynthetic,
}

impl Direction {
    pub fn as_str(self) -> &'static str {
        match self {
            Direction::ClientToServer => "client->server",
            Direction::ServerToClient => "server->client",
            Direction::ServerToClientSynthetic => "server->client synthetic",
        }
    }
}

impl fmt::Display for Direction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

pub fn hex_prefix(bytes: &[u8], limit: usize) -> String {
    let mut out = String::new();
    let shown = bytes.len().min(limit);
    for (index, byte) in bytes.iter().take(shown).enumerate() {
        if index != 0 {
            out.push(' ');
        }
        out.push_str(&format!("{byte:02X}"));
    }
    if bytes.len() > shown {
        out.push_str(" ...");
    }
    out
}
