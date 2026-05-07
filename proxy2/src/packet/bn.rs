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
    Bndr,
    Bnvs,
    Bnvr,
    Bndm,
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
            Some(b"BNDR") => Self::Bndr,
            Some(b"BNVS") => Self::Bnvs,
            Some(b"BNVR") => Self::Bnvr,
            Some(b"BNDM") => Self::Bndm,
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
            _ => Self::Unknown,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Bncs => "BNCS legacy connect start",
            Self::Bncr => "BNCR legacy verifier challenge",
            Self::Bndr => "BNDR EE extended server info response",
            Self::Bnvs => "BNVS legacy verifier response",
            Self::Bnvr => "BNVR legacy verifier result",
            Self::Bndm => "BNDM EE direct disconnect",
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

#[derive(Debug, Clone, Copy)]
pub struct BndrExtendedServerInfo<'a> {
    pub header_word: u16,
    pub details: &'a [u8],
    pub module_description: &'a [u8],
    pub build: &'a [u8],
    pub trailing_word: u16,
}

pub fn parse_bndr_extended_server_info(bytes: &[u8]) -> Option<BndrExtendedServerInfo<'_>> {
    // Decompile-backed shape:
    // EE `CNetLayerInternal::HandleBNDRMessage` rejects packets shorter than
    // 0x0A, then reads three little-endian DWORD-length `CExoString` payloads
    // beginning at offset 6. A final WORD follows the third string. Requiring
    // exact cursor consumption here keeps BNDR from becoming generic BN
    // pass-through while still allowing the EE-valid extended-info response.
    if bytes.get(..4)? != b"BNDR" || bytes.len() < 0x0A {
        return None;
    }

    let header_word = u16::from_le_bytes([bytes[4], bytes[5]]);
    let mut cursor = 6;
    let details = consume_le_u32_string(bytes, &mut cursor)?;
    let module_description = consume_le_u32_string(bytes, &mut cursor)?;
    let build = consume_le_u32_string(bytes, &mut cursor)?;
    let trailing_end = cursor.checked_add(2)?;
    let trailing = bytes.get(cursor..trailing_end)?;
    let trailing_word = u16::from_le_bytes([trailing[0], trailing[1]]);
    if trailing_end != bytes.len() {
        return None;
    }

    Some(BndrExtendedServerInfo {
        header_word,
        details,
        module_description,
        build,
        trailing_word,
    })
}

fn consume_le_u32_string<'a>(bytes: &'a [u8], cursor: &mut usize) -> Option<&'a [u8]> {
    let len_end = cursor.checked_add(4)?;
    let len_bytes = bytes.get(*cursor..len_end)?;
    let len = u32::from_le_bytes([len_bytes[0], len_bytes[1], len_bytes[2], len_bytes[3]]);
    let len = usize::try_from(len).ok()?;
    let value_start = len_end;
    let value_end = value_start.checked_add(len)?;
    let value = bytes.get(value_start..value_end)?;
    *cursor = value_end;
    Some(value)
}
