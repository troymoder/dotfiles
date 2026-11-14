// SPDX-License-Identifier: GPL-3.0-only

//! cabin-ec: the USB-C charger as the Framework laptop's embedded controller
//! sees it, on the system bus for the cabin Battery menu. The kernel only
//! hears of a charger's standard Power Delivery profiles (up to 20 V, so
//! 100 W at most); the EC also agrees the extended ones (EPR, up to 48 V),
//! so a 240 W charger shows as one. The EC takes root and could be told far
//! more than this, so the service answers two questions and nothing else:
//! that, and the CPU package's power (RAPL, also root's alone), since the
//! laptop can't say what it draws while it charges.
//!
//! `cabin-ec --ports` prints what each port reports, to look by hand.

use std::{
    fs::File,
    io,
    os::fd::AsRawFd,
    sync::Mutex,
    time::{Duration, Instant},
};

const BUS_NAME: &str = "dev.troy.Cabin.EC";
const OBJECT_PATH: &str = "/dev/troy/Cabin/EC";
const DEVICE: &str = "/dev/cros_ec";

/// Framework's own EC command: one USB-C port's PD state, as
/// `framework_tool --pdports` shows it.
const EC_CMD_GET_PD_PORT_STATE: u32 = 0x3E23;
/// The EC's answer for a port that isn't there.
const EC_RES_INVALID_PARAM: u32 = 3;
/// Ports asked after. framework_tool stops at four, but the Laptop 16's EC
/// answers for 0–4 (port 2 is a side slot); beyond that it answers junk for
/// 5 and refuses 6 on. The numbers aren't the slots' nor the kernel's.
const PORTS: u8 = 8;

/// The kernel's `struct cros_ec_command`, with room for the reply after it.
#[repr(C)]
struct Command {
    version: u32,
    command: u32,
    outsize: u32,
    insize: u32,
    result: u32,
    data: [u8; 256],
}

/// `CROS_EC_DEV_IOCXCMD`: `_IOWR(0xEC, 0, struct cros_ec_command)`, whose
/// header is 20 bytes.
const CROS_EC_DEV_IOCXCMD: u32 = (3 << 30) | (20 << 16) | (0xEC << 8);

/// A command to the EC: its reply, or the EC's error code.
fn send(ec: &File, command: u32, params: &[u8]) -> io::Result<Result<Vec<u8>, u32>> {
    let mut cmd = Command {
        version: 0,
        command,
        outsize: params.len() as u32,
        insize: 256,
        result: 0xff,
        data: [0; 256],
    };
    cmd.data[..params.len()].copy_from_slice(params);
    // SAFETY: `cmd` is a cros_ec_command with the 256 bytes `insize` promises after it.
    let got = unsafe { libc::ioctl(ec.as_raw_fd(), CROS_EC_DEV_IOCXCMD as _, &mut cmd) };
    if got < 0 {
        return Err(io::Error::last_os_error());
    }
    if cmd.result != 0 {
        return Ok(Err(cmd.result));
    }
    Ok(Ok(cmd.data[..(got as usize).min(cmd.data.len())].to_vec()))
}

/// One port, as the EC tells it.
#[derive(Debug)]
struct Port {
    /// A PD contract is agreed.
    contract: bool,
    /// The laptop takes power here (rather than gives it).
    sink: bool,
    /// The extended power range is in use.
    epr: bool,
    millivolts: u16,
    milliamps: u16,
    /// The port the laptop draws from, of the chargers plugged in.
    active: bool,
    /// The reply as it came, for `--ports`.
    raw: Vec<u8>,
}

impl Port {
    fn watts(&self) -> f64 {
        f64::from(self.millivolts) * f64::from(self.milliamps) / 1e6
    }
}

/// A port's state; `None` when the laptop has no such port. The reply is
/// Framework's `ec_response_get_pd_port_state`: c_state, pd_state,
/// power_role (0 sink), data_role, vconn, epr_active, epr_support,
/// cc_polarity, voltage and current as little-endian u16s, active_port.
fn port(ec: &File, index: u8) -> io::Result<Option<Port>> {
    match send(ec, EC_CMD_GET_PD_PORT_STATE, &[index])? {
        Err(EC_RES_INVALID_PARAM) => Ok(None),
        Err(code) => Err(io::Error::other(format!("the EC answered {code}"))),
        // Past its real ports the Laptop 16's EC answers with 0xff bytes
        // rather than refusing: a state no port can be in is no port.
        Ok(d) if d.len() >= 12 && (d[0] > 6 || d[1] > 1 || d[2] > 1) => Ok(None),
        Ok(d) if d.len() >= 12 => Ok(Some(Port {
            contract: d[1] != 0,
            sink: d[2] == 0,
            epr: d[5] != 0,
            millivolts: u16::from_le_bytes([d[8], d[9]]),
            milliamps: u16::from_le_bytes([d[10], d[11]]),
            active: d.get(12).is_some_and(|a| *a != 0),
            raw: d,
        })),
        Ok(d) => Err(io::Error::other(format!(
            "a short reply: {} bytes",
            d.len()
        ))),
    }
}

/// The power agreed with the charger the laptop runs from, in watts: the
/// one the EC draws from, else the strongest; 0 with none. A port the EC
/// can't speak for is passed over.
fn charger_watts() -> io::Result<f64> {
    let ec = File::open(DEVICE)?;
    let chargers: Vec<Port> = (0..PORTS)
        .filter_map(|i| port(&ec, i).ok().flatten())
        .filter(|p| p.contract && p.sink)
        .collect();
    let pick = chargers.iter().find(|p| p.active).or_else(|| {
        chargers
            .iter()
            .max_by(|a, b| a.watts().total_cmp(&b.watts()))
    });
    Ok(pick.map_or(0.0, Port::watts))
}

/// The CPU package's energy counter, in microjoules.
const RAPL: &str = "/sys/class/powercap/intel-rapl:0/energy_uj";
/// The shortest stretch a CPU reading averages over. The counter is root's
/// because, read finely, it gives away what the CPU is computing (the
/// "Platypus" attack); an average over a second doesn't.
const RAPL_WINDOW: Duration = Duration::from_secs(1);

/// The last counter reading and the watts it gave.
#[derive(Default)]
struct Rapl {
    last: Option<(Instant, u64)>,
    watts: Option<f64>,
}

impl Rapl {
    /// The package's average power since the last reading at least
    /// `RAPL_WINDOW` back; asked sooner, the same answer again.
    fn watts(&mut self) -> io::Result<Option<f64>> {
        let now = Instant::now();
        if self
            .last
            .is_some_and(|(at, _)| now.duration_since(at) < RAPL_WINDOW)
        {
            return Ok(self.watts);
        }
        let energy: u64 = std::fs::read_to_string(RAPL)?
            .trim()
            .parse()
            .map_err(io::Error::other)?;
        // The counter wraps now and then: that stretch is passed over.
        if let Some((at, before)) = self.last.filter(|(_, before)| energy >= *before) {
            self.watts =
                Some((energy - before) as f64 / 1e6 / now.duration_since(at).as_secs_f64());
        }
        self.last = Some((now, energy));
        Ok(self.watts)
    }
}

struct Service {
    rapl: Mutex<Rapl>,
}

#[zbus::interface(name = "dev.troy.Cabin.EC1")]
impl Service {
    /// The power agreed with the USB-C charger in use, in watts; 0 when
    /// nothing gives the laptop power over PD.
    fn charger_watts(&self) -> zbus::fdo::Result<f64> {
        charger_watts().map_err(|err| zbus::fdo::Error::Failed(err.to_string()))
    }

    /// The CPU package's power in watts, averaged over a second or more;
    /// an error on the first call, which only starts the count.
    fn cpu_watts(&self) -> zbus::fdo::Result<f64> {
        let mut rapl = self
            .rapl
            .lock()
            .map_err(|_| zbus::fdo::Error::Failed("poisoned".to_owned()))?;
        match rapl.watts() {
            Ok(Some(watts)) => Ok(watts),
            Ok(None) => Err(zbus::fdo::Error::Failed("not measured yet".to_owned())),
            Err(err) => Err(zbus::fdo::Error::Failed(err.to_string())),
        }
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::args().nth(1).as_deref() == Some("--ports") {
        let ec = File::open(DEVICE)?;
        for index in 0..PORTS {
            match port(&ec, index) {
                Ok(Some(p)) => println!(
                    "port {index}: contract {}, {}{}, EPR {}, {:.3} V × {} mA = {:.1} W   raw {:02x?}",
                    if p.contract { "yes" } else { "no" },
                    if p.sink { "sink" } else { "source" },
                    if p.active { " (active)" } else { "" },
                    if p.epr { "active" } else { "inactive" },
                    f64::from(p.millivolts) / 1000.0,
                    p.milliamps,
                    p.watts(),
                    p.raw
                ),
                Ok(None) => println!("port {index}: not there"),
                Err(err) => println!("port {index}: {err}"),
            }
        }
        println!("charger: {:.1} W", charger_watts()?);
        return Ok(());
    }
    let _conn = zbus::connection::Builder::system()?
        .name(BUS_NAME)?
        .serve_at(
            OBJECT_PATH,
            Service {
                rapl: Mutex::new(Rapl::default()),
            },
        )?
        .build()
        .await?;
    std::future::pending::<()>().await;
    Ok(())
}
