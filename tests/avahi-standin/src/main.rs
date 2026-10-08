//! A stand-in for the part of the Avahi daemon (`org.freedesktop.Avahi`, on
//! the system bus) that Files' Network page uses, so the page can be tried on a
//! private bus in a container that has no multicast network. Test tool only:
//! it is not installed, and it is not Avahi.
//!
//! What it does like Avahi: owns `org.freedesktop.Avahi`; `Server.ServiceBrowserNew`
//! returns the path of a browser object (`/Client1/ServiceBrowser<n>`) and then,
//! to the caller only, sends `ItemNew` for each configured service of the type
//! asked for and `AllForNow`; `Server.ResolveService` answers with the service's
//! host name, address and port; `ServiceBrowser.Free` removes the browser.
//!
//! What it does not: it never touches the network, and it knows only the
//! services in its environment.
//!
//! Switches, by environment: `STANDIN_AVAHI_HOSTS` (services, `;` apart, each
//! `name|type|host|port`), `STANDIN_AVAHI_SLOW_MS` (how long after the first
//! `ItemNew` it says `AllForNow`; 300 by default), `STANDIN_AVAHI_LATE` (one more
//! service, same form) and `STANDIN_AVAHI_LATE_MS` (when it appears, after
//! the browser was made; 3000 by default), `STANDIN_LOG` (a file each call is
//! appended to). The bus is the one in `DBUS_SYSTEM_BUS_ADDRESS`.

use std::io::Write;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;
use zbus::blocking::Connection;
use zbus::zvariant::OwnedObjectPath;

const NAME: &str = "org.freedesktop.Avahi";
const BROWSER_IFACE: &str = "org.freedesktop.Avahi.ServiceBrowser";

fn log(line: &str) {
    if let Some(path) = std::env::var_os("STANDIN_LOG")
        && let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
    {
        let _ = writeln!(f, "{line}");
    }
}

#[derive(Clone, Debug)]
struct Service {
    name: String,
    stype: String,
    host: String,
    port: u16,
}

fn parse(list: &str) -> Vec<Service> {
    list.split(';')
        .filter_map(|e| {
            let mut p = e.split('|');
            Some(Service {
                name: p.next()?.to_string(),
                stype: p.next()?.to_string(),
                host: p.next()?.to_string(),
                port: p.next()?.parse().ok()?,
            })
        })
        .collect()
}

fn env_ms(name: &str, default: u64) -> Duration {
    Duration::from_millis(
        std::env::var(name)
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(default),
    )
}

struct Browser;

#[zbus::interface(name = "org.freedesktop.Avahi.ServiceBrowser")]
impl Browser {
    fn free(&self) {
        log("Browser.Free");
    }
}

struct Server {
    conn: Connection,
    next: AtomicU32,
    services: Vec<Service>,
    late: Option<Service>,
}

#[zbus::interface(name = "org.freedesktop.Avahi.Server")]
impl Server {
    #[zbus(name = "GetVersionString")]
    fn version(&self) -> String {
        "stand-in".into()
    }

    #[zbus(name = "GetState")]
    fn state(&self) -> i32 {
        // AVAHI_SERVER_RUNNING
        2
    }

    #[zbus(name = "ServiceBrowserNew")]
    fn service_browser_new(
        &self,
        _interface: i32,
        _protocol: i32,
        stype: String,
        _domain: String,
        _flags: u32,
    ) -> zbus::fdo::Result<OwnedObjectPath> {
        let n = self.next.fetch_add(1, Ordering::Relaxed);
        let path = format!("/Client1/ServiceBrowser{n}");
        self.conn
            .object_server()
            .at(path.as_str(), Browser)
            .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))?;
        log(&format!("ServiceBrowserNew {stype} -> {path}"));
        let conn = self.conn.clone();
        let found: Vec<Service> = self
            .services
            .iter()
            .filter(|s| s.stype == stype)
            .cloned()
            .collect();
        let late = self.late.clone().filter(|s| s.stype == stype);
        let slow = env_ms("STANDIN_AVAHI_SLOW_MS", 300);
        let late_after = env_ms("STANDIN_AVAHI_LATE_MS", 3000);
        let p = path.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            for s in &found {
                let _ = conn.emit_signal(
                    None::<&str>,
                    p.as_str(),
                    BROWSER_IFACE,
                    "ItemNew",
                    &(0i32, 0i32, s.name.as_str(), s.stype.as_str(), "local", 0u32),
                );
            }
            std::thread::sleep(slow);
            let _ = conn.emit_signal(None::<&str>, p.as_str(), BROWSER_IFACE, "AllForNow", &());
            if let Some(s) = late {
                std::thread::sleep(late_after);
                let _ = conn.emit_signal(
                    None::<&str>,
                    p.as_str(),
                    BROWSER_IFACE,
                    "ItemNew",
                    &(0i32, 0i32, s.name.as_str(), s.stype.as_str(), "local", 0u32),
                );
            }
        });
        OwnedObjectPath::try_from(path).map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
    }

    #[zbus(name = "ResolveService")]
    #[allow(clippy::too_many_arguments, clippy::type_complexity)]
    fn resolve_service(
        &self,
        interface: i32,
        protocol: i32,
        name: String,
        stype: String,
        domain: String,
        _aprotocol: i32,
        _flags: u32,
    ) -> zbus::fdo::Result<(
        i32,
        i32,
        String,
        String,
        String,
        String,
        i32,
        String,
        u16,
        Vec<Vec<u8>>,
        u32,
    )> {
        log(&format!("ResolveService {name} {stype}"));
        let s = self
            .services
            .iter()
            .chain(self.late.iter())
            .find(|s| s.name == name && s.stype == stype)
            .ok_or_else(|| zbus::fdo::Error::Failed("Timeout reached".into()))?;
        Ok((
            interface,
            protocol,
            name,
            stype,
            domain,
            s.host.clone(),
            0,
            "192.0.2.10".to_string(),
            s.port,
            Vec::new(),
            0,
        ))
    }
}

fn main() -> zbus::Result<()> {
    let services = parse(&std::env::var("STANDIN_AVAHI_HOSTS").unwrap_or_default());
    let late = parse(&std::env::var("STANDIN_AVAHI_LATE").unwrap_or_default())
        .into_iter()
        .next();
    let conn = zbus::blocking::connection::Builder::system()?.build()?;
    conn.object_server().at(
        "/",
        Server {
            conn: conn.clone(),
            next: AtomicU32::new(0),
            services,
            late,
        },
    )?;
    conn.request_name(NAME)?;
    log("READY");
    loop {
        std::thread::park();
    }
}
