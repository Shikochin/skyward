//! A credential kept for the starts after this one, sealed under a key
//! stretched from the phone's own serial number and a salt of the file's
//! own: a file read off the flash is not the credential, and one carried to
//! another phone does not open anything there.

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use anyhow::{bail, Context, Result};

/// What the credential is sealed for: a ciphertext made for something else
/// does not open here.
const SEAL: &[u8] = b"skyward unlock v1";
/// How much work stretching the phone's serial number takes, as the powers
/// of two it is written in: a tenth of a second on the phone, which a file
/// read off the flash has to pay again for every guess.
const LOG_N: u8 = 15;
/// How long the salt and the nonce of a sealed secret are.
pub const SALT: usize = 16;
pub const NONCE: usize = 12;

/// `credential` sealed under a key of `serial` and `salt`, with `nonce`.
pub fn seal(serial: &str, salt: &[u8], nonce: &[u8], credential: &[u8]) -> Result<Vec<u8>> {
    let cipher = Aes256Gcm::new_from_slice(&key(serial, salt)?).context("make the cipher")?;
    cipher.encrypt(Nonce::from_slice(nonce), Payload { msg: credential, aad: SEAL }).map_err(|_| anyhow::anyhow!("seal the credential"))
}

/// What [`seal`] sealed, where this phone's serial number and this salt are
/// the ones it was sealed with.
pub fn open(serial: &str, salt: &[u8], nonce: &[u8], sealed: &[u8]) -> Result<Vec<u8>> {
    let cipher = Aes256Gcm::new_from_slice(&key(serial, salt)?).context("make the cipher")?;
    match cipher.decrypt(Nonce::from_slice(nonce), Payload { msg: sealed, aad: SEAL }) {
        Ok(credential) => Ok(credential),
        Err(_) => bail!("the kept credential does not open on this phone"),
    }
}

/// The key the credential is sealed under: the phone's own serial number,
/// stretched with the salt.
fn key(serial: &str, salt: &[u8]) -> Result<[u8; 32]> {
    if salt.len() != SALT {
        bail!("the salt is {} bytes, and {SALT} are wanted", salt.len());
    }
    let params = scrypt::Params::new(LOG_N, 8, 1, 32).map_err(|_| anyhow::anyhow!("the stretching parameters are out of range"))?;
    let mut key = [0u8; 32];
    scrypt::scrypt(serial.as_bytes(), salt, &params, &mut key).map_err(|_| anyhow::anyhow!("the key is not a length the stretching gives"))?;
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_credential_opens_on_the_phone_it_was_kept_on_and_no_other() {
        let (salt, nonce) = ([7u8; SALT], [9u8; NONCE]);
        let sealed = seal("3B162B0073V00000", &salt, &nonce, b"2580").unwrap();
        assert_ne!(&sealed[..], b"2580", "the credential is not kept as it was typed");
        assert_eq!(open("3B162B0073V00000", &salt, &nonce, &sealed).unwrap(), b"2580");
        assert!(open("ANOTHERSERIAL", &salt, &nonce, &sealed).is_err(), "another phone does not open it");
        assert!(open("3B162B0073V00000", &[8u8; SALT], &nonce, &sealed).is_err(), "another salt does not open it");
    }
}
