//! BN/control packet tagging.
//!
//! This file only classifies the top-level four-byte direct-control tag. It
//! does not decide whether the packet is safe to forward; strict validation and
//! semantic translators own that decision. Splitting those concerns keeps the
//! packet inventory broad while still preserving the no-passthrough rule.

#[derive(Debug, Clone)]
pub struct BnPacket<'a> {
    pub bytes: &'a [u8],
    pub tag: BnTag,
}

impl<'a> BnPacket<'a> {
    pub fn parse(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            tag: BnTag::from_bytes(bytes),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BnTag {
    Bncs,
    Bncr,
    Bnvs,
    Bnvr,
    Bndp,
    Bnds,
    Bnk0,
    Bnk1,
    Bnk2,
    Bnk3,
    Bnk4,
    Bnes,
    Bner,
    Bnlm,
    Bnlr,
    Bnxi,
    Bnxr,
    EeDirectCollision,
    Unknown,
}

impl BnTag {
    pub fn from_bytes(bytes: &[u8]) -> Self {
        match bytes.get(..4) {
            Some(b"BNCS") => Self::Bncs,
            Some(b"BNCR") => Self::Bncr,
            Some(b"BNVS") => Self::Bnvs,
            Some(b"BNVR") => Self::Bnvr,
            Some(b"BNDP") => Self::Bndp,
            Some(b"BNDS") => Self::Bnds,
            Some(b"BNK0") => Self::Bnk0,
            Some(b"BNK1") => Self::Bnk1,
            Some(b"BNK2") => Self::Bnk2,
            Some(b"BNK3") => Self::Bnk3,
            Some(b"BNK4") => Self::Bnk4,
            Some(b"BNES") => Self::Bnes,
            Some(b"BNER") => Self::Bner,
            Some(b"BNLM") => Self::Bnlm,
            Some(b"BNLR") => Self::Bnlr,
            Some(b"BNXI") => Self::Bnxi,
            Some(b"BNXR") => Self::Bnxr,
            Some(b"BNDM" | b"BNDR") => Self::EeDirectCollision,
            _ => Self::Unknown,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Bncs => "BNCS legacy connect start",
            Self::Bncr => "BNCR legacy verifier challenge",
            Self::Bnvs => "BNVS legacy verifier response",
            Self::Bnvr => "BNVR legacy verifier result",
            Self::Bndp => "BNDP disconnect reason",
            Self::Bnds => "BNDS disconnect",
            Self::Bnk0 => "BNK0 EE key reset",
            Self::Bnk1 => "BNK1 EE key packet 1",
            Self::Bnk2 => "BNK2 EE key packet 2",
            Self::Bnk3 => "BNK3 EE key packet 3",
            Self::Bnk4 => "BNK4 EE identity",
            Self::Bnes => "BNES EE session enumerate request",
            Self::Bner => "BNER EE session enumerate response",
            Self::Bnlm => "BNLM EE latency/ping request",
            Self::Bnlr => "BNLR EE latency/ping response",
            Self::Bnxi => "BNXI EE extended info request",
            Self::Bnxr => "BNXR EE extended response",
            Self::EeDirectCollision => "EE direct-control collision",
            Self::Unknown => "unknown BN control",
        }
    }
}
