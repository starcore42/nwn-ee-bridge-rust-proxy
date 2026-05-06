use std::ffi::{c_char, c_int, c_void};

use anyhow::{Result, anyhow, bail};
use rand::Rng;

const KX_PUBLIC_KEY_BYTES: usize = 32;
const KX_KEYPAIR_BYTES: usize = 64;
const KX_SESSION_KEYPAIR_BYTES: usize = 64;
const KX_STATE_BYTES: usize = 0xA0;
const KX_PACKET1_BYTES: usize = 32;
const KX_PACKET2_BYTES: usize = 80;
const KX_PACKET3_BYTES: usize = 48;
const SECRETBOX_HEADER_BYTES: usize = 36;
const ENCRYPTED_IDENTITY_HEADER_BYTES: usize = 5;
const PLAYER_CONTEXT: [c_char; 8] = [
    b'p' as c_char,
    b'l' as c_char,
    b'a' as c_char,
    b'y' as c_char,
    b'e' as c_char,
    b'r' as c_char,
    0,
    0,
];

unsafe extern "C" {
    fn hydro_init() -> c_int;
    fn hydro_kx_keygen(static_kp: *mut u8);
    fn hydro_kx_xx_2(
        state: *mut u8,
        packet2: *mut u8,
        packet1: *const u8,
        psk: *const u8,
        static_kp: *const u8,
    ) -> c_int;
    fn hydro_kx_xx_4(
        state: *mut u8,
        kp: *mut u8,
        peer_static_pk: *mut u8,
        packet3: *const u8,
        psk: *const u8,
    ) -> c_int;
    fn hydro_secretbox_encrypt(
        c: *mut u8,
        m: *const c_void,
        mlen: usize,
        msg_id: u64,
        ctx: *const c_char,
        key: *const u8,
    ) -> c_int;
    fn hydro_secretbox_decrypt(
        m: *mut c_void,
        c: *const u8,
        clen: usize,
        msg_id: u64,
        ctx: *const c_char,
        key: *const u8,
    ) -> c_int;
}

#[derive(Debug, Clone)]
pub enum ClientPacket {
    Plain(Vec<u8>),
    ServerResponse(Vec<u8>),
    Consumed,
}

#[derive(Debug, Clone)]
pub struct EeCrypto {
    server_keypair: [u8; KX_KEYPAIR_BYTES],
    kx_state: [u8; KX_STATE_BYTES],
    session_keypair: [u8; KX_SESSION_KEYPAIR_BYTES],
    peer_static_key: [u8; KX_PUBLIC_KEY_BYTES],
    identity: u32,
    stage: u8,
    have_session_keys: bool,
}

impl EeCrypto {
    pub fn new() -> Result<Self> {
        let init = unsafe { hydro_init() };
        if init != 0 {
            bail!("hydro_init failed with {init}");
        }

        let mut server_keypair = [0_u8; KX_KEYPAIR_BYTES];
        unsafe {
            hydro_kx_keygen(server_keypair.as_mut_ptr());
        }
        Ok(Self {
            server_keypair,
            kx_state: [0; KX_STATE_BYTES],
            session_keypair: [0; KX_SESSION_KEYPAIR_BYTES],
            peer_static_key: [0; KX_PUBLIC_KEY_BYTES],
            identity: 0,
            stage: 0,
            have_session_keys: false,
        })
    }

    pub fn preprocess_client_packet(&mut self, bytes: &[u8]) -> Result<ClientPacket> {
        if bytes.starts_with(b"BNK") {
            return self.handle_bnk(bytes).map(ClientPacket::ServerResponse);
        }
        if self.is_identity_encrypted(bytes) {
            return self.decrypt_client_packet(bytes).map(ClientPacket::Plain);
        }
        if bytes.first() == Some(&b'N')
            && bytes.len() >= 1 + KX_PUBLIC_KEY_BYTES + SECRETBOX_HEADER_BYTES
        {
            bail!("unsupported public-key encrypted EE packet");
        }
        Ok(ClientPacket::Plain(bytes.to_vec()))
    }

    pub fn encrypt_server_packet_if_needed(&self, plain: &[u8]) -> Result<Vec<u8>> {
        if !self.have_session_keys || !packet_requires_ee_encryption(plain) {
            return Ok(plain.to_vec());
        }
        let mut cipher = vec![0_u8; plain.len() + SECRETBOX_HEADER_BYTES];
        let result = unsafe {
            hydro_secretbox_encrypt(
                cipher.as_mut_ptr(),
                plain.as_ptr().cast(),
                plain.len(),
                0,
                PLAYER_CONTEXT.as_ptr(),
                self.session_keypair[KX_PUBLIC_KEY_BYTES..].as_ptr(),
            )
        };
        if result != 0 {
            bail!("hydro_secretbox_encrypt failed with {result}");
        }
        let mut out = Vec::with_capacity(ENCRYPTED_IDENTITY_HEADER_BYTES + cipher.len());
        out.push(b'I');
        out.extend_from_slice(&self.identity.to_le_bytes());
        out.extend_from_slice(&cipher);
        Ok(out)
    }

    fn handle_bnk(&mut self, bytes: &[u8]) -> Result<Vec<u8>> {
        match bytes.get(..4) {
            Some(b"BNK0") => {
                self.reset();
                Ok(b"BNK0".to_vec())
            }
            Some(b"BNK1") => self.handle_bnk1(bytes),
            Some(b"BNK3") => self.handle_bnk3(bytes),
            Some(tag) => Err(anyhow!(
                "unsupported EE BNK packet tag={} length={}",
                String::from_utf8_lossy(tag),
                bytes.len()
            )),
            None => bail!("short BNK packet length={}", bytes.len()),
        }
    }

    fn handle_bnk1(&mut self, bytes: &[u8]) -> Result<Vec<u8>> {
        if bytes.len() != 4 + KX_PACKET1_BYTES {
            bail!("BNK1 length mismatch: got {}", bytes.len());
        }
        self.kx_state = [0; KX_STATE_BYTES];
        let mut packet2 = [0_u8; KX_PACKET2_BYTES];
        let result = unsafe {
            hydro_kx_xx_2(
                self.kx_state.as_mut_ptr(),
                packet2.as_mut_ptr(),
                bytes[4..].as_ptr(),
                std::ptr::null(),
                self.server_keypair.as_ptr(),
            )
        };
        if result != 0 {
            self.reset();
            bail!("hydro_kx_xx_2 failed with {result}");
        }
        self.stage = 2;
        let mut out = b"BNK2".to_vec();
        out.extend_from_slice(&packet2);
        Ok(out)
    }

    fn handle_bnk3(&mut self, bytes: &[u8]) -> Result<Vec<u8>> {
        if bytes.len() != 4 + KX_PACKET3_BYTES {
            bail!("BNK3 length mismatch: got {}", bytes.len());
        }
        if self.stage < 2 {
            bail!("BNK3 received before BNK1/BNK2 stage");
        }
        let result = unsafe {
            hydro_kx_xx_4(
                self.kx_state.as_mut_ptr(),
                self.session_keypair.as_mut_ptr(),
                self.peer_static_key.as_mut_ptr(),
                bytes[4..].as_ptr(),
                std::ptr::null(),
            )
        };
        if result != 0 {
            self.reset();
            bail!("hydro_kx_xx_4 failed with {result}");
        }
        self.stage = 3;
        self.have_session_keys = true;
        self.identity = generate_identity();
        let mut out = b"BNK4".to_vec();
        out.extend_from_slice(&self.identity.to_le_bytes());
        Ok(out)
    }

    fn decrypt_client_packet(&self, bytes: &[u8]) -> Result<Vec<u8>> {
        if !self.have_session_keys {
            bail!("encrypted EE packet before session keys");
        }
        let identity = u32::from_le_bytes(bytes[1..5].try_into().expect("identity slice length"));
        if identity != self.identity {
            bail!(
                "encrypted EE packet identity mismatch got=0x{identity:08X} expected=0x{:08X}",
                self.identity
            );
        }
        let cipher = &bytes[ENCRYPTED_IDENTITY_HEADER_BYTES..];
        if cipher.len() < SECRETBOX_HEADER_BYTES {
            bail!("encrypted EE packet cipher too short: {}", cipher.len());
        }
        let mut plain = vec![0_u8; cipher.len() - SECRETBOX_HEADER_BYTES];
        let result = unsafe {
            hydro_secretbox_decrypt(
                plain.as_mut_ptr().cast(),
                cipher.as_ptr(),
                cipher.len(),
                0,
                PLAYER_CONTEXT.as_ptr(),
                self.session_keypair.as_ptr(),
            )
        };
        if result != 0 {
            bail!("hydro_secretbox_decrypt failed with {result}");
        }
        Ok(plain)
    }

    fn is_identity_encrypted(&self, bytes: &[u8]) -> bool {
        bytes.first() == Some(&b'I')
            && bytes.len() >= ENCRYPTED_IDENTITY_HEADER_BYTES + SECRETBOX_HEADER_BYTES
    }

    fn reset(&mut self) {
        self.kx_state = [0; KX_STATE_BYTES];
        self.session_keypair = [0; KX_SESSION_KEYPAIR_BYTES];
        self.peer_static_key = [0; KX_PUBLIC_KEY_BYTES];
        self.identity = 0;
        self.stage = 0;
        self.have_session_keys = false;
    }
}

fn generate_identity() -> u32 {
    loop {
        let value = rand::rng().random::<u32>();
        if value != 0 {
            return value;
        }
    }
}

fn packet_requires_ee_encryption(bytes: &[u8]) -> bool {
    if bytes.is_empty() || bytes.starts_with(b"BNK") {
        return false;
    }
    bytes.starts_with(b"BN") || bytes.first() == Some(&b'M')
}
