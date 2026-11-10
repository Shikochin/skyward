//! The screen lock the recovery keeps, where it is asked to, so that a start
//! opens the data partition without it being typed again. What is kept is the
//! credential sealed under a key stretched from the phone's own serial number
//! and a salt of the file's own: a file read off the flash is not the
//! credential, and a file carried to another phone does not open anything
//! there.

use anyhow::{bail, Context, Result};
use skyward_block::mount;
use std::fs::{self, File};
use std::io::{ErrorKind, Read, Write};
use std::path::Path;

const METADATA: &str = "/metadata";
const FOLDER: &str = "/metadata/skyward";
const FILE: &str = "/metadata/skyward/unlock";
/// What a write goes into first, to be moved over the file whole.
const WRITING: &str = "/metadata/skyward/unlock.new";

/// What the file says it is, so a file of another kind is left alone.
const HEAD: &str = "skyward-unlock 1";
/// How long the salt and the nonce of the sealed credential are.
const SALT: usize = skyward_crypto::keep::SALT;
const NONCE: usize = skyward_crypto::keep::NONCE;

/// Keep `credential`, which opens `user`'s own files, for the starts after
/// this one.
pub fn save(user: u32, credential: &[u8]) -> Result<()> {
    let (mut salt, mut nonce) = ([0u8; SALT], [0u8; NONCE]);
    random(&mut salt)?;
    random(&mut nonce)?;
    let sealed = skyward_crypto::keep::seal(&serial()?, &salt, &nonce, credential)?;
    let text = format!("{HEAD}\nuser {user}\nsalt {}\nnonce {}\nsealed {}\n", hex(&salt), hex(&nonce), hex(&sealed));
    with_metadata(|| {
        skyward_vab::metadata_writable(true)?;
        let wrote = write_file(&text);
        let back = skyward_vab::metadata_writable(false);
        wrote?;
        back
    })
}

/// The credential kept for `user`, where one was kept for them.
pub fn load(user: u32) -> Result<Option<Vec<u8>>> {
    let text = match with_metadata(|| match fs::read_to_string(FILE) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("read {FILE}")),
    })? {
        Some(text) => text,
        None => return Ok(None),
    };
    let kept = Kept::read(&text).with_context(|| format!("{FILE} does not read as a kept credential"))?;
    if kept.user != user {
        return Ok(None);
    }
    match skyward_crypto::keep::open(&serial()?, &kept.salt, &kept.nonce, &kept.sealed) {
        Ok(credential) => Ok(Some(credential)),
        Err(e) => Err(e),
    }
}

/// Forget the kept credential, where one was kept.
pub fn forget() -> Result<()> {
    with_metadata(|| {
        skyward_vab::metadata_writable(true)?;
        let removed = match fs::remove_file(FILE) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e).with_context(|| format!("remove {FILE}")),
        };
        let back = skyward_vab::metadata_writable(false);
        removed?;
        back
    })
}

/// What the file holds.
struct Kept {
    user: u32,
    salt: [u8; SALT],
    nonce: [u8; NONCE],
    sealed: Vec<u8>,
}

impl Kept {
    fn read(text: &str) -> Result<Kept> {
        let mut lines = text.lines();
        if lines.next() != Some(HEAD) {
            bail!("it does not start with {HEAD:?}");
        }
        let (mut user, mut salt, mut nonce, mut sealed) = (None, None, None, None);
        for line in lines {
            let Some((name, value)) = line.split_once(' ') else { continue };
            match name {
                "user" => user = Some(value.trim().parse::<u32>().context("read the user it opens")?),
                "salt" => salt = Some(unhex::<SALT>(value.trim()).context("read the salt")?),
                "nonce" => nonce = Some(unhex::<NONCE>(value.trim()).context("read the nonce")?),
                "sealed" => sealed = Some(unhex_vec(value.trim()).context("read the sealed credential")?),
                _ => {}
            }
        }
        Ok(Kept {
            user: user.context("the file names no user")?,
            salt: salt.context("the file holds no salt")?,
            nonce: nonce.context("the file holds no nonce")?,
            sealed: sealed.context("the file holds no sealed credential")?,
        })
    }
}

/// The serial number the credential is sealed with, which no other phone has.
fn serial() -> Result<String> {
    skyward_sys::prop("ro.serialno").context("the phone says no serial number")
}

fn random(out: &mut [u8]) -> Result<()> {
    File::open("/dev/urandom").and_then(|mut f| f.read_exact(out)).context("read /dev/urandom")
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex_vec(text: &str) -> Result<Vec<u8>> {
    if text.len() % 2 != 0 {
        bail!("{text:?} is not a whole number of bytes");
    }
    (0..text.len() / 2)
        .map(|i| u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).with_context(|| format!("read byte {i} of {text:?}")))
        .collect()
}

fn unhex<const N: usize>(text: &str) -> Result<[u8; N]> {
    let bytes = unhex_vec(text)?;
    <[u8; N]>::try_from(bytes.as_slice()).map_err(|_| anyhow::anyhow!("{text:?} is {} bytes, and {N} are wanted", bytes.len()))
}

/// Write the file whole: into a file beside it, flushed, then moved over it.
fn write_file(text: &str) -> Result<()> {
    fs::create_dir_all(FOLDER).with_context(|| format!("make {FOLDER}"))?;
    let mut file = File::create(WRITING).with_context(|| format!("make {WRITING}"))?;
    file.write_all(text.as_bytes()).with_context(|| format!("write {WRITING}"))?;
    file.sync_all().with_context(|| format!("write {WRITING}"))?;
    fs::rename(WRITING, FILE).with_context(|| format!("move {WRITING} over {FILE}"))?;
    File::open(FOLDER).and_then(|folder| folder.sync_all()).with_context(|| format!("write {FOLDER}"))
}

/// Do `work` on /metadata, mounted read-only for the moment where it is not
/// mounted, as in fastboot, and taken away again after.
fn with_metadata<T>(work: impl FnOnce() -> Result<T>) -> Result<T> {
    let at = Path::new(METADATA);
    if mount::is_mount_point(at)? {
        return work();
    }
    mount::mount_ro(&Path::new(skyward_block::BY_NAME).join("metadata"), at)?;
    let done = work();
    mount::umount(at)?;
    done
}
