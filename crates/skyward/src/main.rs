use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand};
use skyward_config::{ramdisk, DeviceConfig};
use skyward_sys::{bcb, Power};
use skyward_vab::SnapshotState;
use std::path::{Path, PathBuf};

#[cfg(target_os = "android")]
mod backup;
#[cfg(target_os = "android")]
mod boot;
#[cfg(target_os = "android")]
mod decrypt;
#[cfg(target_os = "android")]
mod filework;
#[cfg(target_os = "android")]
mod haptics;
#[cfg(target_os = "android")]
mod input;
#[cfg(target_os = "android")]
mod merge;
#[cfg(target_os = "android")]
mod partwork;
#[cfg(target_os = "android")]
mod pty;
#[cfg(target_os = "android")]
mod readings;
#[cfg(target_os = "android")]
mod relabel;
#[cfg(target_os = "android")]
mod remember;
#[cfg(target_os = "android")]
mod secure;
#[cfg(target_os = "android")]
mod services;
#[cfg(target_os = "android")]
mod settings;
#[cfg(target_os = "android")]
mod systems;
#[cfg(target_os = "android")]
mod torch;
#[cfg(target_os = "android")]
mod ui;
#[cfg(target_os = "android")]
mod updates;
#[cfg(target_os = "android")]
mod wipe;
#[cfg(target_os = "android")]
mod wlan;

#[derive(Parser)]
#[command(name = "skyward", version, about = "Skyward recovery")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Print what the About page shows, plus a partition summary.
    Info {
        /// Device configuration, e.g. devices/nezha.toml.
        #[arg(long)]
        config: String,
    },
    /// Print the suffix of the slot the phone runs from and, where the
    /// phone's boot control runs, what it holds of each slot.
    Slot,
    /// List modules in dependency load order from a source directory.
    Modules {
        #[arg(long, default_value = "/vendor/lib/modules")]
        source: String,
    },
    /// Load kernel modules and their dependencies from the configured
    /// sources; with no names, the configuration's own list.
    Load {
        #[arg(long, default_value = "/skyward/device.toml")]
        config: String,
        names: Vec<String>,
    },
    /// Run as the recovery service (what init starts as /system/bin/recovery).
    Recovery,
    /// Reboot through init: system, recovery, fastboot, bootloader, edl, or
    /// off.
    Reboot { target: String },
    /// Print the bootloader control block in misc.
    Bcb,
    /// Print the logical partition metadata in super.
    Lp {
        /// The super partition, or an image of its start.
        #[arg(long = "super", default_value = "/dev/block/by-name/super")]
        super_dev: PathBuf,
        /// Metadata slot: 0 for _a, 1 for _b. Defaults to the booted slot.
        #[arg(long)]
        slot: Option<u32>,
    },
    /// Tell what a package or image is, the way the installer reads it.
    Identify { file: PathBuf },
    /// Discard all a block device holds and write zeros over its start, as
    /// a factory reset erases the data partition.
    #[cfg(target_os = "android")]
    Discard { device: PathBuf },
    /// Make a filesystem on a block device, f2fs or ext4, as a factory
    /// reset makes /metadata afresh.
    #[cfg(target_os = "android")]
    Mkfs { device: PathBuf, filesystem: String },
    /// Map the booted slot's partitions from super and mount them read-only.
    Mount,
    /// Unmount what `mount` mounted and remove its device-mapper devices.
    Umount,
    /// Print where an update stands: its state, the slot it came from, each
    /// snapshot as libsnapshot keeps it and as the kernel maps it, and the
    /// kernel's snapshot targets.
    Update,
    /// Merge the update into the slot the phone runs from, starting the
    /// merge where it has not begun, and follow it to its end.
    #[cfg(target_os = "android")]
    Merge,
    /// Keep the configuration's services that this phone runs going until
    /// stopped.
    #[cfg(target_os = "android")]
    Services {
        #[arg(long, default_value = "/skyward/device.toml")]
        config: String,
    },
    /// List the display connectors, their modes, CRTCs and planes.
    #[cfg(target_os = "android")]
    Kms {
        #[arg(long, default_value = "/dev/dri/card0")]
        card: String,
    },
    /// Play the vibration motor's click once, or the ticks of the screen
    /// going off by the power key, saying how strongly and for how long.
    #[cfg(target_os = "android")]
    Haptic {
        #[arg(long, default_value = "/skyward/device.toml")]
        config: String,
        /// Where on the strength slider, from 0 at its left end to 1 at its
        /// right.
        #[arg(long, default_value_t = skyward_ui::app::SLIDER_START)]
        position: f32,
        /// Play the ticks of the screen going off instead of the click.
        #[arg(long)]
        ticks: bool,
    },
    /// Save what the panel shows, as raw RGBA files cap-000.rgba onwards.
    #[cfg(target_os = "android")]
    Capture {
        out: PathBuf,
        #[arg(long, default_value_t = 1)]
        count: u32,
        /// Pause between captures, in milliseconds.
        #[arg(long, default_value_t = 100)]
        interval: u64,
        #[arg(long, default_value = "/dev/dri/card0")]
        card: String,
    },
    /// Set the system clock from the RTC and the offset Android keeps on
    /// persist, as the recovery does when it starts, and print the time in
    /// China, the first of the time zones the settings offer.
    #[cfg(target_os = "android")]
    Clock,
    /// Say whether a service has registered with the phone's service
    /// directory, such as android.hardware.security.keymint.IKeyMintDevice/default.
    #[cfg(target_os = "android")]
    Service { name: String },
    /// Report the phone's key service and, given a key directory Android
    /// wrote, unwrap the key it holds and say how long it is.
    #[cfg(target_os = "android")]
    Keymint {
        #[arg(long, default_value = "/metadata/vold/metadata_encryption/key")]
        key: PathBuf,
        /// Report instead what the key service keeps under this name, such
        /// as profile_key_name_decrypt_999.
        #[arg(long)]
        alias: Option<String>,
    },
    /// Say what the phone's credential service answers, which takes no
    /// credential and changes nothing.
    #[cfg(target_os = "android")]
    Gatekeeper,
    /// Say what the phone's weaver says of itself, which reads no slot and
    /// changes nothing.
    #[cfg(target_os = "android")]
    Weaver,
    /// Open the data partition with the key the phone keeps for it, check
    /// it and mount it writable as the phone does, and open each user's
    /// files that open; the owner's shared storage then shows at /sdcard.
    #[cfg(target_os = "android")]
    Data {
        /// What a user would type at their screen lock, where one is set.
        /// A wrong one is counted against that user by the phone's secure
        /// world, the same as a wrong try on the screen itself.
        #[arg(long)]
        credential: Option<String>,
        /// Close the data partition instead, as a factory reset does before
        /// it erases it: every other program holding it ends first.
        #[arg(long, conflicts_with = "credential")]
        close: bool,
    },
    /// Drive the panel and touchscreen with the recovery UI.
    #[cfg(target_os = "android")]
    Ui {
        #[arg(long, default_value = "/skyward/device.toml")]
        config: std::path::PathBuf,
        #[arg(long, default_value = "/skyward/MiSansVF.ttf")]
        font: std::path::PathBuf,
        /// The terminal's monospaced typeface.
        #[arg(long, default_value = "/skyward/KirkMono-Regular.ttf")]
        mono: std::path::PathBuf,
        /// Open on the unlock page instead of the home page.
        #[arg(long)]
        unlock: bool,
        /// After each frame, also write it as raw RGBA to this file.
        #[arg(long)]
        dump: Option<std::path::PathBuf>,
    },
}

/// A panic is said in the kernel log before it takes the recovery down: the
/// phone shows the pages again a moment later, and this is the only place
/// that says why it did not.
#[cfg(target_os = "android")]
fn report_panics() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = skyward_sys::kmsg(&format!("skyward panicked: {info}"));
        previous(info);
    }));
}

fn main() -> Result<()> {
    #[cfg(target_os = "android")]
    report_panics();
    // init starts the recovery service by path with no arguments.
    let started_as = std::env::args_os().next().unwrap_or_default();
    let name = Path::new(&started_as).file_name().unwrap_or_default();
    if name == "recovery" {
        return recovery();
    }
    // Install scripts run the commands linked to this binary by their names.
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if let Some(code) = skyward_install::commands::run(name, &args) {
        std::process::exit(code);
    }
    match Cli::parse().cmd {
        Cmd::Info { config } => info(&config),
        Cmd::Slot => slot(),
        Cmd::Modules { source } => modules(&source),
        Cmd::Load { config, names } => {
            let cfg = DeviceConfig::load(&config)?;
            let names = if names.is_empty() { cfg.modules.load.clone() } else { names };
            for path in load_modules(&cfg, &names)? {
                println!("{}", path.display());
            }
            Ok(())
        }
        Cmd::Recovery => recovery(),
        Cmd::Reboot { target } => {
            let p = Power::parse(&target).ok_or_else(|| anyhow!("unknown target {target:?}"))?;
            skyward_sys::power(p)
        }
        Cmd::Bcb => {
            let b = bcb::read(&bcb::misc())?;
            println!("command = {:?}", b.command);
            println!("args    = {:?}", b.args);
            println!("request = {:?}", b.request());
            Ok(())
        }
        #[cfg(target_os = "android")]
        Cmd::Discard { device } => skyward_block::erase::discard(&device),
        #[cfg(target_os = "android")]
        Cmd::Mkfs { device, filesystem } => skyward_block::erase::make(&device, &filesystem, &[], false),
        Cmd::Lp { super_dev, slot } => lp(&super_dev, slot),
        Cmd::Update => update(),
        #[cfg(target_os = "android")]
        Cmd::Merge => {
            let slot = booted_slot()?;
            let start = skyward_vab::standing(slot)? == skyward_vab::Standing::Waiting;
            let end = merge::run(slot, start, &mut |m| println!("{m:?}"))?;
            println!("ended {end:?}");
            Ok(())
        }
        Cmd::Identify { file } => identify(&file),
        Cmd::Mount => {
            let mounts = skyward_vab::mount_partitions(booted_slot()?)?;
            for d in &mounts.damaged {
                println!("damaged: {d}");
            }
            for m in &mounts.partitions {
                println!("{:<14} {:<16} {:<18} {}", m.partition, m.device.display(), m.mount_point.display(), m.filesystem);
            }
            Ok(())
        }
        Cmd::Umount => skyward_vab::unmount_partitions(booted_slot()?),
        #[cfg(target_os = "android")]
        Cmd::Services { config } => {
            let props = skyward_info::installed_props(booted_slot()?)?;
            let mut services = DeviceConfig::load(&config)?.services;
            services.retain(|s| s.runs_on(&props));
            services::start(&services, secure::libraries()?.as_deref())?;
            loop {
                std::thread::park();
            }
        }
        #[cfg(target_os = "android")]
        Cmd::Kms { card } => {
            print!("{}", skyward_ui::surface::kms::report(&card)?);
            Ok(())
        }
        #[cfg(target_os = "android")]
        Cmd::Haptic { config, position, ticks } => {
            let haptics = DeviceConfig::load(&config)?.haptics.context("the configuration describes no vibration motor")?;
            haptics::probe(&haptics, position, ticks)
        }
        #[cfg(target_os = "android")]
        Cmd::Capture { out, count, interval, card } => {
            std::fs::create_dir_all(&out).with_context(|| format!("create {}", out.display()))?;
            for i in 0..count {
                let t0 = std::time::Instant::now();
                let c = skyward_ui::surface::kms::capture(&card)?;
                let path = out.join(format!("cap-{i:03}.rgba"));
                std::fs::write(&path, &c.data).with_context(|| format!("write {}", path.display()))?;
                println!("{} {}x{} in {} ms", path.display(), c.w, c.h, t0.elapsed().as_millis());
                std::thread::sleep(std::time::Duration::from_millis(interval));
            }
            Ok(())
        }
        #[cfg(target_os = "android")]
        Cmd::Clock => {
            let ms = skyward_sys::clock::set_from_rtc()?;
            println!("{}", skyward_ui::fmt::datetime(skyward_ui::Lang::En, (ms / 1000) as i64, 0));
            Ok(())
        }
        #[cfg(target_os = "android")]
        Cmd::Service { name } => {
            let binder = skyward_binder::driver::Binder::open()?;
            let described = if skyward_binder::service::declared(&binder, &name)? { "described" } else { "not described" };
            let running = match skyward_binder::service::find(&binder, &name)? {
                Some(_) => "registered",
                None => "not registered",
            };
            println!("{name} is {described} and {running}");
            Ok(())
        }
        #[cfg(target_os = "android")]
        Cmd::Keymint { key, alias } => {
            let binder = skyward_binder::driver::Binder::open()?;
            let service = skyward_crypto::keymint::KeyMint::open(&binder)?;
            let hw = service.hardware()?;
            println!("keymint {} version {} at security level {} by {}", hw.name, hw.version, hw.security_level, hw.author);
            if let Some(alias) = alias {
                let held = skyward_crypto::keystore2::key_blob(Path::new(skyward_crypto::keystore2::DATABASE), &alias)?;
                println!("{alias} is a key of {} bytes", held.len());
                for (tag, value) in service.authorizations(&held, &[])? {
                    println!("  tag {tag} = {value} ({value:#x})");
                }
                match service.usable(&held, &[]) {
                    Ok(()) => println!("the key service unwraps it and will use it"),
                    Err(e) => println!("the key service will not use it: {e:#}"),
                }
                return Ok(());
            }
            let stored = skyward_crypto::keystore::StoredKey::load(&key)?;
            for (tag, value) in stored.authorizations(&service)? {
                println!("  tag {tag} = {value}");
            }
            let raw = stored.unwrap(skyward_crypto::keystore::Unwrap::Hardware(&service))?;
            println!("{} holds a key of {} bytes", key.display(), raw.len());
            Ok(())
        }
        #[cfg(target_os = "android")]
        Cmd::Gatekeeper => {
            let (binder, hardware) = (skyward_binder::driver::Binder::open()?, skyward_binder::driver::Binder::hardware()?);
            let gatekeeper = skyward_crypto::gatekeeper::Gatekeeper::open(&binder, &hardware)?;
            println!("the credential service answers for {}", gatekeeper.serves()?);
            match gatekeeper.version() {
                Ok(Some(version)) => println!("it serves version {version} of that interface"),
                Ok(None) => println!("it numbers its version in its own name"),
                Err(e) => println!("it will not say which version it serves: {e:#}"),
            }
            // A credential belonging to no one, which the secure world
            // cannot recognise and counts against nobody. What comes back
            // says whether the service takes a check at all, apart from
            // whether it recognises what it is given.
            match gatekeeper.verify(NOBODY, &[], &[]) {
                Ok(_) => println!("it took a credential belonging to no one"),
                Err(e) => println!("asked about a credential belonging to no one it says: {e:#}"),
            }
            Ok(())
        }
        #[cfg(target_os = "android")]
        Cmd::Weaver => {
            let binder = skyward_binder::driver::Binder::open()?;
            let c = skyward_crypto::weaver::Weaver::open(&binder)?.config()?;
            println!("the weaver has {} slots, keeps keys of {} bytes and values of {} bytes", c.slots, c.key_bytes, c.value_bytes);
            Ok(())
        }
        #[cfg(target_os = "android")]
        Cmd::Data { close: true, .. } => {
            wipe::release_data()?;
            println!("the data partition is closed");
            Ok(())
        }
        #[cfg(target_os = "android")]
        Cmd::Data { credential, close: false } => {
            use skyward_crypto::data;
            let binder = skyward_binder::driver::Binder::open()?;
            let service = skyward_crypto::keymint::KeyMint::open(&binder)?;
            let opening = data::open_without_credential(&service)?;
            match &opening.mounted {
                Some(mounted) => {
                    if let Some(checked) = &mounted.checked {
                        print!("{}", checked.said);
                        println!("the check of {} ended with {}", mounted.mapped.display(), checked.status);
                    }
                    println!("{} holds {} at {}", mounted.mapped.display(), mounted.filesystem, data::MOUNT);
                }
                None => println!("{} is mounted already", data::MOUNT),
            }
            let mut closed = opening.closed;
            let asked = closed.iter().find(|c| !c.made_up).map(|c| c.user);
            if let (Some(credential), Some(user)) = (&credential, asked) {
                closed = data::unlock(&service, &closed, user, credential.as_bytes())?;
            }
            for c in &closed {
                match &c.why {
                    Some(why) => println!("user {}'s own files stay closed: {why}", c.user),
                    None => println!("user {}'s own files stay closed", c.user),
                }
            }
            if closed.is_empty() {
                println!("every user's own files can be read");
            }
            Ok(())
        }
        #[cfg(target_os = "android")]
        Cmd::Ui { config, font, mono, unlock, dump } => {
            let cfg = DeviceConfig::load(&config)?;
            let start = if unlock { skyward_ui::app::Screen::Decrypt } else { skyward_ui::app::Screen::Home };
            ui::run(&cfg, &font, &mono, bcb::Request::default(), start, dump.as_deref())
        }
    }
}

fn booted_slot() -> Result<skyward_vab::Slot> {
    skyward_vab::current_slot()?.context("not an A/B device")
}

fn lp(super_dev: &Path, slot: Option<u32>) -> Result<()> {
    let slot = match slot {
        Some(n) => n,
        None => booted_slot()?.number(),
    };
    let md = skyward_lp::read(super_dev, slot)?;
    let g = md.geometry;
    println!("slot {slot}: version {}.{}, flags {:#x}", md.major_version, md.minor_version, md.header_flags);
    println!(
        "geometry: {} metadata slots of {} bytes, logical block {} bytes",
        g.metadata_slot_count, g.metadata_max_size, g.logical_block_size
    );
    for d in &md.damaged {
        println!("damaged: {d}");
    }
    for d in &md.block_devices {
        println!(
            "device {}: first sector {}, {} bytes, alignment {} offset {}",
            d.partition_name, d.first_logical_sector, d.size, d.alignment, d.alignment_offset
        );
    }
    for gr in &md.groups {
        println!("group {}: max {} bytes", gr.name, gr.maximum_size);
    }
    for p in &md.partitions {
        println!(
            "partition {} group {} attributes {:#x}, {} bytes",
            p.name, md.groups[p.group].name, p.attributes, p.bytes()
        );
        let mut start = 0;
        for e in &p.extents {
            let end = start + e.sectors() - 1;
            match *e {
                skyward_lp::Extent::Linear { device, sector, .. } => {
                    println!("  {start} .. {end} linear {} {sector}", md.block_devices[device].partition_name)
                }
                skyward_lp::Extent::Zero { .. } => println!("  {start} .. {end} zero"),
            }
            start = end + 1;
        }
    }
    Ok(())
}

/// Load `names` from the module sources the configuration lists.
fn load_modules(cfg: &DeviceConfig, names: &[String]) -> Result<Vec<PathBuf>> {
    let dirs = cfg.modules.sources.iter().map(|s| skyward_modules::source_dir(s)).collect::<Result<Vec<_>>>()?;
    skyward_modules::load(&dirs, names)
}

/// Start the recovery's own logd where it needs libraries of the installed
/// system, make the phone's firmware reachable to its drivers, mount the
/// partitions its own programs and drivers use where it mounts them, load
/// the configured kernel modules, hand the files the phone's init hands
/// over as it boots to their accounts, and start the phone's own programs,
/// read from the partitions just mounted: on the 13 Ultra, the touchscreen
/// driver and the touch processing. Of the programs the configuration
/// names, only those this phone's build of the hardware runs are started.
#[cfg(target_os = "android")]
fn bring_up(cfg: &DeviceConfig, slot: skyward_vab::Slot) -> Result<()> {
    let nodes = skyward_sys::ueventd::apply()?;
    skyward_sys::kmsg(&format!("{nodes} device nodes given the owner their phone describes"))?;
    let props = skyward_info::installed_props(slot)?;
    adopt_installed_version(&props)?;
    // Without logd the phone's own programs still run, and only what they
    // log is lost, so why it did not start goes to the kernel log.
    if let Some(logd) = &cfg.logd {
        if let Err(e) = start_logd(logd) {
            skyward_sys::kmsg(&format!("logd not started: {e:#}"))?;
        }
    }
    let services: Vec<_> = cfg.services.iter().filter(|s| s.runs_on(&props)).cloned().collect();
    // The drivers ask for their firmware by name, and the kernel looks only
    // in folders of its own unless told to pass on what it does not find
    // there to ueventd, which looks in the recovery's firmware folders.
    std::fs::write(FIRMWARE_FALLBACK, "1").with_context(|| format!("write 1 to {FIRMWARE_FALLBACK}"))?;
    if let Some(firmware) = &cfg.firmware {
        mount_firmware(&firmware.partition)?;
    }
    for name in &cfg.partitions.mounted {
        let at = skyward_block::fstab::mount(name)?;
        skyward_sys::kmsg(&format!("{name} is mounted on {}", at.display()))?;
    }
    for path in load_modules(cfg, &cfg.modules.load)? {
        skyward_sys::kmsg(&format!("loaded {}", path.display()))?;
    }
    if !cfg.owners.is_empty() {
        let handed = hand_over(&cfg.owners)?;
        skyward_sys::kmsg(&format!("{handed} of the phone's own files handed to the accounts its init gives them"))?;
    }
    let hardware: Vec<String> = services.iter().filter_map(|s| s.ready_hardware_service.clone()).collect();
    if !hardware.is_empty() {
        secure::prepare_hardware_directory(&hardware)?;
        skyward_sys::kmsg("the directory for the phone's hardware services can run")?;
    }
    // Only a program that has to register with the service directory needs
    // the directory to know the phone describes it. The WLAN's supplicant
    // registers whenever the WLAN is turned on, and the directory can no
    // longer start again by then: the key service registers with it below,
    // and nothing would register that again.
    let mut registering: Vec<String> = services.iter().flat_map(|s| s.ready_service.iter().chain(&s.registers).cloned()).collect();
    registering.extend(cfg.wlan.iter().map(|w| w.service.clone()));
    let files: Vec<String> = services.iter().filter_map(|s| s.description.clone()).collect();
    if !registering.is_empty() || !files.is_empty() {
        let described = secure::publish_descriptions(&registering, &files)?;
        skyward_sys::kmsg(&format!("{described} of the phone's own service descriptions published"))?;
    }
    let libraries = secure::libraries()?;
    if let Some(dir) = &libraries {
        skyward_sys::kmsg(&format!("the phone's own libraries are at {}", dir.display()))?;
    }
    services::start(&services, libraries.as_deref())?;
    agree_on_the_notes_key(&registering)
}

/// Give each file the phone's own init hands over as it boots to the
/// account it names, with the mode it gives, as its chown and chmod lines
/// do. The files are the ones the modules just loaded made, which the
/// phone's own programs open as those accounts.
#[cfg(target_os = "android")]
fn hand_over(owners: &[skyward_config::Owners]) -> Result<usize> {
    use skyward_sys::ueventd;
    use std::os::unix::fs::PermissionsExt;
    let known = ueventd::accounts(&ueventd::PASSWD_FILES.iter().chain(&ueventd::GROUP_FILES).copied().collect::<Vec<_>>());
    let id = |name: &str| ueventd::account(&known, name).with_context(|| format!("the phone has no account named {name}"));
    let mut handed = 0;
    for run in owners {
        let (uid, gid) = (id(&run.user)?, id(&run.group)?);
        for path in &run.paths {
            // A node the modules this phone's kernel loaded did not make is
            // left out: the programs that open it are the ones that matter,
            // and they are started either way.
            if !std::path::Path::new(path).exists() {
                let _ = skyward_sys::kmsg(&format!("{path} is not there to give to {}:{}", run.user, run.group));
                continue;
            }
            std::os::unix::fs::chown(path, Some(uid), Some(gid)).with_context(|| format!("give {path} to {}:{}", run.user, run.group))?;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(run.mode))
                .with_context(|| format!("give {path} the mode {:o}", run.mode))?;
            handed += 1;
        }
    }
    Ok(handed)
}

/// The folder the recovery's own programs load their libraries from.
#[cfg(target_os = "android")]
const RECOVERY_LIBRARIES: &str = "/system/lib64";

/// How long logd may take to say it is ready once started.
#[cfg(target_os = "android")]
const LOGD_READY: std::time::Duration = std::time::Duration::from_secs(5);

/// Give the recovery's own logd the libraries of the installed system it
/// loads, and start it at once rather than on init's next try, so that it
/// keeps what the phone's own programs log from their start. Each library
/// is copied beside the recovery's own under a name of its own and then
/// renamed into place, so a logd an earlier run of the recovery service
/// started keeps the copy it has open.
#[cfg(target_os = "android")]
fn start_logd(logd: &skyward_config::Logd) -> Result<()> {
    for library in &logd.libraries {
        let from = Path::new(library);
        let to = Path::new(RECOVERY_LIBRARIES).join(from.file_name().expect("the configuration names each library's file"));
        let mut part = to.clone().into_os_string();
        part.push(".part");
        std::fs::copy(from, &part).with_context(|| format!("copy {library} to {}", Path::new(&part).display()))?;
        std::fs::rename(&part, &to).with_context(|| format!("put {} in place", to.display()))?;
    }
    skyward_sys::set_prop("ctl.start", "logd")?;
    skyward_sys::wait_prop("logd.ready", "true", LOGD_READY)?;
    skyward_sys::kmsg(&format!("logd runs with {} libraries of the installed system", logd.libraries.len()))
}

/// Have the parts of the secure world that keep keys agree on the key they
/// sign their notes with, where the phone has any that do. Android's key
/// service has them agree as the phone starts; until they have, the
/// credential service cannot write a note the key service will take, and
/// the keys the phone guards behind a screen lock stay unusable.
#[cfg(target_os = "android")]
fn agree_on_the_notes_key(registered: &[String]) -> Result<()> {
    let interface = format!("{}/", skyward_crypto::sharedsecret::INTERFACE);
    let names: Vec<String> = registered.iter().filter(|n| n.starts_with(&interface)).cloned().collect();
    if names.is_empty() {
        return Ok(());
    }
    let binder = skyward_binder::driver::Binder::open()?;
    let agreed = skyward_crypto::sharedsecret::agree(&binder, &names)?;
    skyward_sys::kmsg(&format!("{agreed} of the secure world agreed on the key it signs its notes with"))
}

/// A slot of the credential service that belongs to no user, which the
/// phone numbers its own from nothing upwards.
#[cfg(target_os = "android")]
const NOBODY: u32 = u32::MAX;

/// The switch that has the kernel pass the firmware requests it cannot
/// answer itself on to ueventd.
#[cfg(target_os = "android")]
const FIRMWARE_FALLBACK: &str = "/proc/sys/kernel/firmware_config/force_sysfs_fallback";

/// Where the firmware partition is shown as well: the recovery's own
/// ueventd looks in the image folder inside it.
#[cfg(target_os = "android")]
const FIRMWARE_SHOWN: &str = "/firmware";

/// Mount the partition holding the firmware images where and as the
/// phone's own table mounts it, which is where the loaders of the secure
/// world's apps look for theirs, and show it where the recovery's ueventd
/// looks for the rest. Both stay when the recovery service starts again.
#[cfg(target_os = "android")]
fn mount_firmware(partition: &str) -> Result<()> {
    let shown = Path::new(FIRMWARE_SHOWN);
    if skyward_block::mount::is_mount_point(shown)? {
        return Ok(());
    }
    let at = skyward_block::fstab::mount(partition)?;
    std::fs::create_dir_all(shown).with_context(|| format!("make {}", shown.display()))?;
    skyward_block::mount::bind(&at, shown)?;
    skyward_sys::kmsg(&format!("{partition} is mounted on {}, shown at {}", at.display(), shown.display()))
}

/// What the phone's own secure services read to tell the secure world which
/// build is running: the system's version and patch level, and the vendor
/// partition's patch level. A recovery image carries its own, older build's
/// values; the secure world then sees keys the installed system made as
/// made under a build it does not know, and refuses them. The recovery's
/// property files leave these out so that they can be set here instead.
#[cfg(target_os = "android")]
const BUILD_VERSION: [&str; 3] = ["ro.build.version.release", "ro.build.version.security_patch", "ro.vendor.build.security_patch"];

/// Set each of [`BUILD_VERSION`] to the installed system's value. An earlier
/// run of the recovery service may have set it already, and init sets such
/// a property only once.
#[cfg(target_os = "android")]
fn adopt_installed_version(props: &std::collections::HashMap<String, String>) -> Result<()> {
    for name in BUILD_VERSION {
        let value = props.get(name).with_context(|| format!("the installed system says no {name}"))?;
        if skyward_sys::prop(name).as_ref() != Some(value) {
            skyward_sys::set_prop(name, value)?;
        }
        skyward_sys::kmsg(&format!("{name} is the installed system's {value}"))?;
    }
    Ok(())
}

/// Let the recovery run what its policy does not allow, where the
/// configuration asks for it: a recovery carries no framework of the
/// installed system's, and the policy a recovery image was built with was
/// built for the phone that image was made on.
fn set_permissive() -> bool {
    use std::fs;
    const ENFORCE: &str = "/sys/fs/selinux/enforce";
    let already = fs::read_to_string(ENFORCE).is_ok_and(|v| v.trim() == "0");
    let now = already || (fs::write(ENFORCE, b"0").is_ok() && fs::read_to_string(ENFORCE).is_ok_and(|v| v.trim() == "0"));
    let _ = skyward_sys::kmsg(&format!(
        "the recovery runs {}, {}",
        if now { "permissive" } else { "enforcing" },
        if now { "as the configuration asks" } else { "and the policy did not give way" }
    ));
    now
}

/// The recovery service. Its own folder is made and its log kept there from
/// the start, and adb comes up first so a failure later on can be
/// inspected, file transfer joining it
/// once the screen starts; then the boot request in misc is read. A
/// request whose work
/// goes on stays in misc until the work is done; any other is cleared at
/// once, so that any reboot from here boots the system. Failures go to the
/// kernel log and still end the process, which init then starts again.
fn recovery() -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let run = || -> Result<()> {
        // Open to the programs the recovery runs as other users, the
        // supplicant among them, whatever the umask.
        std::fs::create_dir_all(skyward_sys::RUN).with_context(|| format!("make {}", skyward_sys::RUN))?;
        std::fs::set_permissions(skyward_sys::RUN, std::fs::Permissions::from_mode(0o755))
            .with_context(|| format!("open {} to the recovery's other users", skyward_sys::RUN))?;
        skyward_sys::keep_log(Path::new(skyward_sys::LOG))?;
        let cfg = DeviceConfig::load(Path::new("/").join(ramdisk::CONFIG))?;
        // A policy built for the phone this image was made on need not allow
        // the programs of another build, which a recovery runs with no
        // framework of the installed system's to fall back on. Where it gives
        // way, the port is set up again: what the policy refused is asked for
        // once more now that it is not refused.
        let permissive = cfg.device.permissive && set_permissive();
        // Parts of the phone's own recovery expect a recovery program of the
        // phone maker's and may restart the phone when it does not answer:
        // they are frozen before this recovery takes over, which leaves them
        // running as far as init is concerned.
        #[cfg(target_os = "android")]
        for name in &cfg.device.freeze {
            let at = format!("/system/bin/{name}");
            let line = match crate::services::running(&at) {
                Some(pid) => match skyward_sys::freeze(pid) {
                    Ok(()) => format!("froze {name} (pid {pid}), which expects the phone's own recovery"),
                    Err(e) => format!("{name} (pid {pid}) did not freeze: {e:#}"),
                },
                None => format!("{name} was not running, so nothing was frozen"),
            };
            skyward_sys::kmsg(&line)?;
        }
        // The phone's own recovery puts its port into host mode, where it
        // offers a stick a computer would; a port that is to be served by a
        // computer is put back into device mode first, as the same port is
        // when the system runs.
        #[cfg(target_os = "android")]
        for (file, value) in [
            ("/sys/class/oplus_chg/usb/otg_switch", "0"),
            ("/sys/bus/platform/devices/a600000.dwc3/mode", "peripheral"),
            ("/sys/bus/platform/devices/a600000.ssusb/mode", "peripheral"),
        ] {
            if Path::new(file).exists() {
                match std::fs::write(file, value) {
                    Ok(()) => skyward_sys::kmsg(&format!("wrote {value} to {file}"))?,
                    Err(e) => skyward_sys::kmsg(&format!("{file} did not take {value}: {e}"))?,
                }
            }
        }
        // A recovery that ended with a function of its own in the port took
        // the port with it; going through "none" takes that function off.
        if permissive {
            if let Err(e) = skyward_sys::usb::switch("adb", std::time::Duration::from_secs(5)) {
                let _ = skyward_sys::kmsg(&format!("the port did not come back to adb: {e:#}"));
                skyward_sys::set_prop("sys.usb.config", "adb")?;
            }
        } else {
            if skyward_sys::prop("sys.usb.state").is_some_and(|s| s != "adb") {
                skyward_sys::usb::set("none", std::time::Duration::from_secs(5))?;
            }
            skyward_sys::set_prop("sys.usb.config", "adb")?;
        }
        // The phone's block devices get their names as ueventd walks them,
        // which can take a moment after the recovery service starts: a run
        // that started before then ended and was started again.
        // The port is handed to a computer once more after the phone's own
        // init has had its say about it. A port bound while the phone still
        // held it its own way is one no computer sees: the sideload page,
        // which takes the port through "none" before it asks for its own
        // function, is seen at once.
        #[cfg(target_os = "android")]
        {
            std::thread::sleep(std::time::Duration::from_secs(2));
            if let Err(e) = skyward_sys::usb::switch("adb", std::time::Duration::from_secs(5)) {
                let _ = skyward_sys::kmsg(&format!("the port did not come back to adb: {e:#}"));
                skyward_sys::set_prop("sys.usb.config", "adb")?;
            } else {
                let _ = skyward_sys::kmsg("the port is handed to a computer as adb again");
            }
        }
        let misc = bcb::misc();
        let mut waited = 0;
        while !misc.exists() && waited < 50 {
            std::thread::sleep(std::time::Duration::from_millis(100));
            waited += 1;
        }
        let message = bcb::read(&misc)?;
        let request = message.request();
        if !request.work.lasts() {
            bcb::clear(&misc)?;
        }
        skyward_sys::kmsg(&format!(
            "{} {} recovery up, misc command {:?} args {:?}, asking for {:?}",
            cfg.device.codename,
            skyward_info::VERSION,
            message.command,
            message.args,
            request.work
        ))?;
        if !request.unknown.is_empty() {
            skyward_sys::kmsg(&format!("misc arguments this recovery does not know: {:?}", request.unknown))?;
        }
        if request.work == bcb::Work::Exit {
            return skyward_sys::power(if request.shutdown_after { Power::Off } else { Power::System });
        }
        // An update the system asked for starts counting its starts in misc,
        // as Android's own recovery counts them, so that one a restart cuts
        // off is taken up again as a retry; one that follows the kernel
        // stopping is not started at all.
        #[cfg(target_os = "android")]
        if let bcb::Work::Update(update) = &request.work {
            if update.retry == 0 && !ui::panicked() {
                bcb::write(&misc, &message.with_retry(1))?;
            }
        }
        // The kernel starts the clock from the RTC, days after 1970 on these
        // phones; the screen and the files written still work without the
        // real time, so a failure goes to the kernel log.
        #[cfg(target_os = "android")]
        match skyward_sys::clock::set_from_rtc() {
            Ok(ms) => skyward_sys::kmsg(&format!("clock set to {} ms from the RTC and persist", ms))?,
            Err(e) => skyward_sys::kmsg(&format!("clock not set: {e:#}"))?,
        }
        // The kernel starts every core at its top frequency, and keeps idle
        // ones from powering down, until the phone's own system boots and
        // changes that; left so, the phone drains its battery with the
        // screen off. The recovery works either way, so a failure goes to
        // the kernel log.
        #[cfg(target_os = "android")]
        match skyward_sys::cpu::run_as_system() {
            Ok(done) => skyward_sys::kmsg(&format!("the CPUs run as the system runs them: {}", done.join(", ")))?,
            Err(e) => skyward_sys::kmsg(&format!("the CPUs run as the kernel started them: {e:#}"))?,
        }
        #[cfg(target_os = "android")]
        {
            use skyward_ui::app::Screen;
            // fastbootd rewrites super, which it cannot do while the slot's
            // partitions are mapped and mounted, so fastboot runs without
            // them and without what runs from them. So does work on whole
            // partitions the system asks for, as in Android's own recovery:
            // erasing them, which may take super with them, and undoing
            // shared blocks, which writes to the slot's own.
            // The screen comes up before the phone's partitions do, so that
            // the wait for them is an animation rather than a dark screen.
            let booting = crate::boot::start(cfg.display.density);
            if !matches!(request.work, bcb::Work::Fastboot | bcb::Work::WipeAb { .. } | bcb::Work::UnshareBlocks) {
                prepare(&cfg, booted_slot()?)?;
                let report = boot_report(&cfg);
                let _ = std::fs::write("/skyward/run/boot.log", &report);
                let _ = skyward_sys::kmsg(&format!("boot report of {} lines written", report.lines().count()));
            }
            // The pages take the screen from the animation, which lets go of
            // it first.
            drop(booting);
            let root = Path::new("/");
            ui::run(&cfg, &root.join(ramdisk::FONT), &root.join(ramdisk::MONO), request, Screen::Home, None)
        }
        #[cfg(not(target_os = "android"))]
        {
            anyhow::bail!("the recovery service runs only on Android")
        }
    };
    run().inspect_err(|e| {
        let _ = skyward_sys::kmsg(&format!("recovery failed: {e:#}"));
    })
}

/// What the recovery service found on the phone before the pages started,
/// written for the page that shows it: a recovery no computer and no finger
/// can reach has no other way to say why its parts did not come up.
#[cfg(target_os = "android")]
fn boot_report(cfg: &DeviceConfig) -> String {
    use std::fmt::Write as _;
    use std::path::Path;
    let mut out = String::new();
    let _ = writeln!(out, "skyward {} — boot report", skyward_info::VERSION);
    let _ = writeln!(out, "device: {} ({})", cfg.device.codename, cfg.device.model);
    let _ = writeln!(
        out,
        "boot: reason={:?} sys={:?} uptime={}s",
        skyward_sys::prop("ro.boot.bootreason"), skyward_sys::prop("sys.boot.reason"),
        std::fs::read_to_string("/proc/uptime").ok().and_then(|u| u.split('.').next().and_then(|s| s.parse::<u64>().ok())).unwrap_or(0)
    );
    let bcb = skyward_sys::bcb::read(&skyward_sys::bcb::misc()).map(|m| format!("{:?} {:?}", m.command, m.args)).unwrap_or_else(|e| format!("unreadable: {e}"));
    let _ = writeln!(out, "misc: {bcb}");
    let phoenix = crate::services::running("/system/bin/phoenix_recovery").map_or_else(|| "no".to_string(), |pid| format!("pid {pid}"));
    let _ = writeln!(out, "phoenix: {phoenix}  battery={}C", std::fs::read_to_string("/sys/class/power_supply/battery/temp").map(|t| format!("{:.1}", t.trim().parse::<f64>().unwrap_or(0.0) / 10.0)).unwrap_or_default());

    let enforce = std::fs::read_to_string("/sys/fs/selinux/enforce").unwrap_or_default();
    let _ = writeln!(out, "selinux: enforce={}", enforce.trim());
    let mounted = |at: &str| skyward_block::mount::is_mount_point(Path::new(at)).unwrap_or(false);
    let _ = writeln!(
        out,
        "mounted: vendor={} odm={} vendor_dlkm={} system_dlkm={} metadata={}",
        mounted("/vendor"), mounted("/odm"), mounted("/vendor_dlkm"), mounted("/system_dlkm"), mounted("/metadata")
    );
    let there = |at: &str| if Path::new(at).exists() { "yes" } else { "NO" };
    // The touch program is wherever the configuration says it is: the one
    // the image carries, or the installed system's.
    let touch = cfg.services.iter().find(|s| s.path.contains("touch")).map_or("/odm/bin/hw/vendor-oplus-hardware-touch-V2-hbp5-service", |s| s.path.as_str());
    let _ = writeln!(
        out,
        "touch: service={} at={} hbp_core={} hbp0={} sockets={}",
        there(touch), touch, there("/dev/hbp_core"), there("/dev/hbp0"), there("/dev/socket/hidl2touchdaemon_client")
    );
    let running = |at: &str| crate::services::running(at).map_or_else(|| "no".to_string(), |pid| format!("pid {pid}"));
    let _ = writeln!(
        out,
        "services: touch={} qseecomd={} keymint={}",
        running(touch),
        running("/vendor/bin/qseecomd"),
        running("/vendor/bin/hw/android.hardware.security.onekeymint-service-qti")
    );
    let mut states: Vec<String> = Vec::new();
    if let Ok(dir) = std::fs::read_dir("/sys/class/udc") {
        for e in dir.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let state = std::fs::read_to_string(e.path().join("state")).unwrap_or_default();
            states.push(format!("{name}={}", state.trim()));
        }
    }
    states.sort();
    let otg = std::fs::read_to_string("/sys/class/oplus_chg/usb/otg_switch").unwrap_or_default();
    let role = std::fs::read_to_string("/sys/class/typec/port0/role").unwrap_or_default();
    let _ = writeln!(out, "port: otg_switch={} typec_role={} udc=[{}]", otg.trim(), role.trim(), states.join(","));
    let state = states.first().and_then(|s| s.rsplit_once('=')).map_or(String::new(), |(_, v)| v.to_string());
    let _ = writeln!(
        out,
        "usb: config={:?} state={:?} adbd={:?} udc={}",
        skyward_sys::prop("sys.usb.config"), skyward_sys::prop("sys.usb.state"), skyward_sys::prop("init.svc.adbd"), state.trim()
    );
    let modules = std::fs::read_to_string("/proc/modules").unwrap_or_default();
    let loaded: Vec<&str> = modules.lines().filter(|l| l.contains("hbp") || l.contains("tp_")).map(|l| l.split(' ').next().unwrap_or("")).collect();
    let _ = writeln!(out, "modules: {}", if loaded.is_empty() { "none of touch".to_string() } else { loaded.join(",") });
    let has = |name: &str| if modules.lines().any(|l| l.starts_with(name)) { "yes" } else { "NO" };
    // The gadget init sets up for adb, as far as a recovery can see it: the
    // host only sees what these say.
    if let Ok(dir) = std::fs::read_dir("/config/usb_gadget/g1/functions") {
        let mut functions: Vec<String> = dir.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        functions.sort();
        let at = |name: &str| std::fs::read_to_string(format!("/config/usb_gadget/g1/{name}")).map(|s| s.trim().to_string()).unwrap_or_default();
        let f1 = std::fs::read_link("/config/usb_gadget/g1/configs/b.1/f1").map_or_else(|_| "none".to_string(), |p| p.display().to_string());
        let _ = writeln!(
            out,
            "gadget: vid={} pid={} udc={} f1={f1} functions=[{}] serial={:?}",
            at("idVendor"), at("idProduct"), at("UDC"), functions.join(","), skyward_sys::prop("ro.serialno")
        );
    }
    let _ = writeln!(
        out,
        "usb gadget: ffs_mounted={} configfs={} usb_f_fs={} libcomposite={} ffs_ready={:?} ep0={}",
        mounted("/dev/usb-ffs/adb"), there("/config/usb_gadget/g1"), has("usb_f_fs"), has("libcomposite"),
        skyward_sys::prop("sys.usb.ffs.ready"), there("/dev/usb-ffs/adb/ep0")
    );
    let log = std::fs::read_to_string(skyward_sys::LOG).unwrap_or_default();
    let tail: Vec<&str> = log.lines().rev().filter(|l| {
        !l.trim_start().starts_with('/')
            && ["not brought up", "not mounted", "missing", "not started", "failed", "did not", "denied", "has no account"].iter().any(|k| l.contains(k))
    }).take(8).map(|l| l.trim_start_matches(|c: char| c == '[' || c.is_ascii_digit() || c == '.' || c == ' ' || c == ']')).collect();
    let _ = writeln!(out, "-- last steps that did not go --");
    for line in tail.iter().rev() {
        let _ = writeln!(out, "{line}");
    }
    // What the touch program itself said, which is the first thing a recovery
    // that runs the phone's own program has to go by.
    let name = Path::new(touch).file_name().map_or_else(|| "touch".to_string(), |n| n.to_string_lossy().into_owned());
    let said = std::fs::read_to_string(format!("{}/{name}.log", skyward_sys::RUN)).unwrap_or_default();
    let lines: Vec<&str> = said.lines().rev().take(10).collect();
    let _ = writeln!(out, "-- {name} said ({} lines) --", said.lines().count());
    for line in lines.iter().rev() {
        let cut: String = line.chars().take(100).collect();
        let _ = writeln!(out, "{cut}");
    }
    // The kernel's own record of a start that went wrong, if the phone
    // keeps one: mounted here, since a recovery's init does not.
    if !skyward_block::mount::is_mount_point(Path::new("/sys/fs/pstore")).unwrap_or(false) {
        let _ = std::fs::create_dir_all("/sys/fs/pstore");
        let _ = skyward_block::mount::mount(Path::new("pstore"), Path::new("/sys/fs/pstore"), "pstore", &[]);
    }
    if let Ok(dir) = std::fs::read_dir("/sys/fs/pstore") {
        let names: Vec<String> = dir.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        let _ = writeln!(out, "-- pstore: {} --", if names.is_empty() { "none".to_string() } else { names.join(",") });
        for name in &names {
            if let Ok(text) = std::fs::read_to_string(format!("/sys/fs/pstore/{name}")) {
                let last: Vec<&str> = text.lines().filter(|l| {
                    ["panic", "watchdog", "reboot", "thermal", "shutdown", "Power", "reset"].iter().any(|k| l.contains(k))
                }).rev().take(4).collect();
                for line in last.iter().rev() {
                    let cut: String = line.chars().take(90).collect();
                    let _ = writeln!(out, "{cut}");
                }
            }
        }
    }
    if let Ok(klog) = skyward_sys::klog() {
        let hits: Vec<&str> = klog.lines().rev().filter(|l| {
            ["avc:", "avc_denied", "skyward:", "module", "usb", "hbp", "touch", "keymint", "qseecom", "selinux"].iter().any(|k| l.contains(k))
        }).take(12).collect();
        let _ = writeln!(out, "-- kernel log --");
        for line in hits.iter().rev() {
            let cut: String = line.chars().take(90).collect();
            let _ = writeln!(out, "{cut}");
        }
    }
    out
}

/// Point the loader the recovery's own programs ask for by name at the one
/// the installed system carries: a recovery image built beside a system
/// image has its programs ask for the bootstrap loader, and a recovery
/// ramdisk need not carry one. Without it, the filesystem checks the
/// recovery ships cannot run at all.
#[cfg(target_os = "android")]
fn prepare_early_loader() -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    const LOADER: &str = "/system/bin/bootstrap/linker64";
    let at = Path::new(LOADER);
    if at.exists() {
        return Ok(());
    }
    let dir = at.parent().expect("the loader is in a folder");
    std::fs::create_dir_all(dir).with_context(|| format!("make {}", dir.display()))?;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o755)).with_context(|| format!("open {} to every user", dir.display()))?;
    let from = ["/mnt/system/system/bin/bootstrap/linker64", "/mnt/system/system/bin/linker64"]
        .into_iter()
        .map(Path::new)
        .find(|p| p.is_file())
        .with_context(|| format!("the installed system carries no loader for {LOADER}"))?;
    std::os::unix::fs::symlink(from, at).with_context(|| format!("point {} at {}", at.display(), from.display()))?;
    skyward_sys::kmsg(&format!("{} is the installed system's {}", at.display(), from.display()))
}

/// Map and mount the slot's partitions from super, then bring up what runs
/// from them. Recovery has to stay usable when super cannot be mounted, as
/// after it is wiped, so the reason goes to the kernel log instead.
#[cfg(target_os = "android")]
fn prepare(cfg: &DeviceConfig, slot: skyward_vab::Slot) -> Result<()> {
    match skyward_vab::mount_partitions(slot) {
        Ok(mounts) => {
            for d in &mounts.damaged {
                skyward_sys::kmsg(&format!("super: {d}"))?;
            }
            for m in &mounts.partitions {
                skyward_sys::kmsg(&format!("mounted {} on {} ({})", m.partition, m.mount_point.display(), m.filesystem))?;
            }
            // The filesystem checks the recovery ships ask for a loader the
            // ramdisk need not carry; the installed system has it.
            if let Err(e) = prepare_early_loader() {
                skyward_sys::kmsg(&format!("the loader the recovery's own programs ask for is not there: {e:#}"))?;
            }
            // Without them the screen still shows and the keys still work,
            // so this reason goes to the kernel log as well.
            if let Err(e) = bring_up(cfg, slot) {
                skyward_sys::kmsg(&format!("devices not brought up: {e:#}"))?;
            }
        }
        Err(e) => skyward_sys::kmsg(&format!("partitions not mounted: {e:#}"))?,
    }
    Ok(())
}

fn info(config: &str) -> Result<()> {
    let cfg = DeviceConfig::load(config)?;
    let i = skyward_info::collect(&cfg)?;
    println!("model         = {} ({})", i.model, i.codename);
    println!("soc           = {:?}", i.soc);
    println!("cpu           = {} cores, max {:?} kHz", i.cpu.cores, i.cpu.max_khz);
    println!("ram           = {} KiB", i.ram_kib);
    println!("panel         = {:?}", i.panel);
    println!("screen        = {:?} in", i.screen);
    println!("os_version    = {:?}", i.os_version);
    println!("android       = {:?}", i.android);
    println!("patch         = {:?}", i.security_patch);
    println!("kernel        = {}", i.kernel);
    println!("storage       = {} bytes, free {:?}", i.storage.total, i.storage.free);
    println!("battery       = {:?}", i.battery);
    println!("boot          = {:?}", i.boot);
    println!("recovery      = {}", skyward_info::VERSION);
    match i.slot {
        Some(s) => println!("current_slot  = {}", s.suffix()),
        None => println!("current_slot  = (not A/B)"),
    }
    let snap = i.snapshot;
    println!("snapshot      = {snap:?}");
    match snap {
        SnapshotState::None => {}
        SnapshotState::Unknown => println!("  -> /metadata is not mounted; dynamic-partition writes stay refused"),
        _ => println!("  -> dynamic-partition writes are refused while a snapshot is outstanding"),
    }
    let parts = skyward_block::list_by_name(skyward_block::BY_NAME)?;
    println!("partitions    = {} under {}", parts.len(), skyward_block::BY_NAME);
    Ok(())
}

fn identify(path: &Path) -> Result<()> {
    use skyward_install::package::{self, Package};
    let file = std::fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
    match package::identify(std::io::BufReader::new(file))? {
        Package::Update(u) => {
            println!("an A/B update to {}", u.version().unwrap_or("an unnamed version"));
            println!("payload.bin at byte {}, {} bytes", u.payload_offset, u.payload_size);
            for key in ["ota-type", "pre-device", "post-build", "post-timestamp", "post-security-patch-level", "pre-build"] {
                if let Some(v) = u.metadata.get(key) {
                    println!("{key:<26} {v}");
                }
            }
            if u.wipes() {
                println!("all user data is erased once it is in, after the user agrees");
            }
        }
        Package::Image(kind) => println!("{kind:?}, for {}", kind.partitions().join(" or ")),
        Package::Script => println!("a zip that installs itself with a script"),
        Package::System(c) => {
            println!("the system {} {} ({}), to install beside the installed one", c.manifest.name, c.manifest.version, c.manifest.id);
            println!("a boot image of {} bytes, a root image of {} bytes ({}), starting {}", c.boot, c.root, c.manifest.filesystem, c.manifest.init);
            println!("its root takes at least {} bytes", c.least());
            if c.init_boot {
                println!("it brings its own init_boot image");
            }
        }
        Package::Unknown => println!("nothing this recovery installs"),
    }
    Ok(())
}

fn slot() -> Result<()> {
    match skyward_vab::current_slot()? {
        Some(s) => println!("{}", s.suffix()),
        None => println!("(not A/B)"),
    }
    #[cfg(target_os = "android")]
    {
        use skyward_vab::boot::BootControl;
        let (binder, hardware) = (skyward_binder::driver::Binder::open()?, skyward_binder::driver::Binder::hardware()?);
        let opened = BootControl::open(&binder, &hardware);
        match opened {
            Ok(control) => {
                let slots = control.slots()?;
                println!("next {}", slots.active.suffix());
                for slot in [skyward_vab::Slot::A, skyward_vab::Slot::B] {
                    let k = slot.number() as usize;
                    println!("{} bootable {} successful {}", slot.suffix(), slots.bootable[k], slots.successful[k]);
                }
                println!("update {:?}", control.merge_status()?);
            }
            Err(e) => println!("{e:#}"),
        }
    }
    Ok(())
}

fn update() -> Result<()> {
    let slot = booted_slot()?;
    println!("standing {:?}", skyward_vab::standing(slot)?);
    for kind in ["snapshot", "snapshot-merge", "user"] {
        println!("target {kind} {:?}", skyward_block::dm::target_version(kind)?);
    }
    let Some(snaps) = skyward_vab::snapshots::Snapshots::lock(false)? else {
        println!("no update was ever applied");
        return Ok(());
    };
    let u = snaps.update()?;
    println!("state {:?} phase {:?} failure {:?}", snaps.state()?, u.merge_phase, u.merge_failure_code);
    println!("snapuserd {} userspace {} allocated {} total {} metadata {}", u.using_snapuserd, u.userspace_snapshots, u.sectors_allocated, u.total_sectors, u.metadata_sectors);
    println!("from {:?}, {:?} for {}, built {:?}", snaps.source()?, snaps.side(slot)?, slot.suffix(), u.source_build_fingerprint);
    println!("needed {}", snaps.needed(slot)?);
    for name in snaps.list()? {
        let s = snaps.status(&name)?;
        println!(
            "{name}: {:?} device {} snapshot {} cow {} + {} allocated {} metadata {} snapuserd {} old {}",
            s.part_state()?,
            s.device_size,
            s.snapshot_size,
            s.cow_partition_size,
            s.cow_file_size,
            s.sectors_allocated,
            s.metadata_sectors,
            s.using_snapuserd,
            s.old_partition_size
        );
        for dev in [name.clone(), format!("{name}-base"), format!("{name}-cow"), format!("{name}-cow-img")] {
            if skyward_block::dm::status(&dev)?.is_some() {
                for line in skyward_block::dm::table_status(&dev)? {
                    println!("  {dev}: {} {}+{} {}", line.kind, line.start, line.sectors, line.text);
                }
            }
        }
    }
    Ok(())
}

fn modules(source: &str) -> Result<()> {
    let graph = skyward_modules::parse_dep(source)?;
    let order = graph.load_order(&[]);
    println!("{} modules in dependency order", order.len());
    for m in &order {
        println!("  {m}");
    }
    Ok(())
}
