//! Credential encoding for the native EAPOL validation utility.
//! No Ethernet transport, credential files, or system configuration lives here.
use crate::protocol;
use anyhow::{Result, ensure};

const MAX_USERNAME: usize = 256;
const MAX_PASSWORD: usize = 1024;

pub fn wipe(bytes: &mut [u8]) {
    for byte in bytes {
        // Prevent the compiler from dropping a clear immediately before free.
        unsafe { std::ptr::write_volatile(byte, 0) };
    }
}

pub struct Credentials {
    pub username: String,
    password: Vec<u8>,
}
impl Drop for Credentials {
    fn drop(&mut self) {
        wipe(&mut self.password);
    }
}
impl Credentials {
    pub fn answer(
        &self,
        source: [u8; 6],
        destination: [u8; 6],
        packet: &protocol::Packet<'_>,
    ) -> Result<Option<Vec<u8>>> {
        protocol::answer(
            source,
            destination,
            packet,
            &protocol::gbk(&self.username)?,
            &self.password,
        )
    }
    pub fn new(username: &str, password: &str) -> Result<Self> {
        ensure!(
            !username.is_empty() && username.len() <= MAX_USERNAME,
            "invalid NetID length"
        );
        ensure!(
            !username.contains('\0') && !password.contains('\0'),
            "credentials contain NUL"
        );
        protocol::gbk(username)?;
        let password = protocol::gbk(password)?;
        ensure!(
            !password.is_empty() && password.len() <= MAX_PASSWORD,
            "invalid password length"
        );
        Ok(Self {
            username: username.into(),
            password,
        })
    }
}
