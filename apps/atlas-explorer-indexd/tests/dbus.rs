//! The service on a private session bus: `dbus-daemon` is started by the test
//! (own config, own socket, own service folder), the binary is run directly or
//! activated, and Search, Status, NotifyChanged, Refresh and StatusChanged are
//! called over D-Bus. Skipped, with a message, when `dbus-daemon` is missing,
//! unless `ATLAS_EXPLORER_REQUIRE_BUS_TESTS=1`. Everything lives under
//! `$ATLAS_TEST_DIR`; the user's real bus and files are never touched.

use atlas_file_index::testdir::Scratch;
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};
use zbus::blocking::{Connection, Proxy, connection};
use zbus::zvariant::{OwnedValue, Value};

const NAME: &str = "net.eterneon.atlas.explorer.Search";
const PATH: &str = "/net/eterneon/atlas/explorer/Search";
const IFACE: &str = "net.eterneon.atlas.explorer.Search1";
const BIN: &str = env!("CARGO_BIN_EXE_atlas-explorer-indexd");

type Hit = (String, String, String, String, String, i64, u64, f64);

struct Bus {
    _t: Scratch,
    dir: PathBuf,
    root: PathBuf,
    env: Vec<(String, String)>,
    daemon: Child,
    address: String,
    service: Option<Child>,
}

fn skip_or_fail(why: &str) -> Option<Bus> {
    if std::env::var("ATLAS_EXPLORER_REQUIRE_BUS_TESTS").as_deref() == Ok("1") {
        panic!("D-Bus tests are required but cannot run: {why}");
    }
    eprintln!(
        "SKIPPED: D-Bus test, {why} (set ATLAS_EXPLORER_REQUIRE_BUS_TESTS=1 to make this an error)"
    );
    None
}

/// A private bus. `activate`: the service is started by the bus (a service
/// file in the bus's own folder) instead of by the test.
fn bus(name: &str, scan_delay_ms: u64, activate: bool) -> Option<Bus> {
    let t = Scratch::new(name);
    let dir = fs::canonicalize(&t.0).unwrap();
    let root = dir.join("tree");
    fs::create_dir_all(&root).unwrap();
    for sub in [
        "home/.config/atlas-explorer",
        "home/.cache",
        "home/.local/share",
        "services",
    ] {
        fs::create_dir_all(dir.join(sub)).unwrap();
    }
    fs::write(
        dir.join("home/.config/atlas-explorer/indexrc"),
        format!("[Index]\nRoots={}\n", root.display()),
    )
    .unwrap();
    // the socket path must be short enough for sockaddr_un
    let sock = std::env::temp_dir().join(format!("atlas-idx-bus-{}-{name}", std::process::id()));
    let _ = fs::remove_file(&sock);
    let conf = format!(
        "<!DOCTYPE busconfig PUBLIC \"-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN\" \"http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd\">\n\
<busconfig><type>session</type><listen>unix:path={}</listen><auth>EXTERNAL</auth><servicedir>{}</servicedir>\n\
<policy context=\"default\"><allow send_destination=\"*\" eavesdrop=\"true\"/><allow eavesdrop=\"true\"/><allow own=\"*\"/></policy></busconfig>\n",
        sock.display(),
        dir.join("services").display()
    );
    let conf_path = dir.join("bus.conf");
    fs::write(&conf_path, conf).unwrap();
    let env: Vec<(String, String)> = vec![
        ("HOME".into(), dir.join("home").display().to_string()),
        (
            "XDG_CONFIG_HOME".into(),
            dir.join("home/.config").display().to_string(),
        ),
        (
            "XDG_CACHE_HOME".into(),
            dir.join("home/.cache").display().to_string(),
        ),
        (
            "XDG_DATA_HOME".into(),
            dir.join("home/.local/share").display().to_string(),
        ),
        (
            "ATLAS_EXPLORER_TEST_SCAN_DELAY_MS".into(),
            scan_delay_ms.to_string(),
        ),
        ("ATLAS_EXPLORER_LOG".into(), "debug".into()),
    ];
    if activate {
        fs::write(
            dir.join("services/net.eterneon.atlas.explorer.Search.service"),
            format!("[D-BUS Service]\nName={NAME}\nExec={BIN}\n"),
        )
        .unwrap();
    }
    let mut cmd = Command::new("dbus-daemon");
    cmd.arg(format!("--config-file={}", conf_path.display()))
        .args(["--print-address", "--nofork"])
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    for (k, v) in &env {
        cmd.env(k, v); // activated services inherit the bus's environment
    }
    let mut daemon = match cmd.spawn() {
        Ok(d) => d,
        Err(e) => return skip_or_fail(&format!("dbus-daemon cannot be started ({e})")),
    };
    // read the address on a thread so a daemon that prints nothing fails the test after 10 s
    let out = daemon.stdout.take().unwrap();
    let (atx, arx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut line = String::new();
        let _ = BufReader::new(out).read_line(&mut line);
        let _ = atx.send(line);
    });
    let line = match arx.recv_timeout(Duration::from_secs(10)) {
        Ok(l) => l,
        Err(_) => {
            let _ = daemon.kill();
            let _ = daemon.wait();
            panic!("dbus-daemon printed no address within 10 s");
        }
    };
    let address = line.trim().to_string();
    assert!(address.starts_with("unix:"), "bus address: {address:?}");
    Some(Bus {
        _t: t,
        dir,
        root,
        env,
        daemon,
        address,
        service: None,
    })
}

impl Bus {
    fn conn(&self) -> Connection {
        connection::Builder::address(self.address.as_str())
            .unwrap()
            .build()
            .unwrap()
    }

    fn start_service(&mut self) {
        let mut cmd = Command::new(BIN);
        cmd.env("DBUS_SESSION_BUS_ADDRESS", &self.address)
            .stderr(Stdio::inherit());
        for (k, v) in &self.env {
            cmd.env(k, v);
        }
        self.service = Some(cmd.spawn().unwrap());
    }

    fn proxy<'a>(&self, conn: &'a Connection) -> Proxy<'a> {
        Proxy::new(conn, NAME, PATH, IFACE).unwrap()
    }

    fn wait_for_name(&self, conn: &Connection) {
        let dbus = zbus::blocking::fdo::DBusProxy::new(conn).unwrap();
        let end = Instant::now() + Duration::from_secs(10);
        while Instant::now() < end {
            if dbus
                .name_has_owner(NAME.try_into().unwrap())
                .unwrap_or(false)
            {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("the service never claimed {NAME}");
    }

    fn service_pid(&self, conn: &Connection) -> Option<u32> {
        let dbus = zbus::blocking::fdo::DBusProxy::new(conn).ok()?;
        let name: zbus::names::BusName = NAME.try_into().ok()?;
        dbus.get_connection_unix_process_id(name).ok()
    }
}

impl Drop for Bus {
    fn drop(&mut self) {
        // stop the service first (SIGTERM: it writes its snapshot and exits), then the bus
        let pid = self.service.as_ref().map(Child::id).or_else(|| {
            let c = connection::Builder::address(self.address.as_str())
                .ok()?
                .build()
                .ok()?;
            self.service_pid(&c)
        });
        if let Some(pid) = pid {
            // SAFETY: plain kill(2) on a process this test started.
            unsafe {
                libc::kill(pid as i32, 15);
            }
            let end = Instant::now() + Duration::from_secs(5);
            while Path::new(&format!("/proc/{pid}")).exists()
                && !zombie(pid)
                && Instant::now() < end
            {
                if let Some(s) = self.service.as_mut() {
                    let _ = s.try_wait();
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            // SAFETY: as above.
            unsafe {
                libc::kill(pid as i32, 9);
            }
        }
        if let Some(mut s) = self.service.take() {
            let _ = s.wait();
        }
        let _ = self.daemon.kill();
        let _ = self.daemon.wait();
        let _ = &self.dir;
    }
}

fn zombie(pid: u32) -> bool {
    fs::read_to_string(format!("/proc/{pid}/stat"))
        .map(|s| s.contains(") Z"))
        .unwrap_or(true)
}

fn status(p: &Proxy) -> HashMap<String, OwnedValue> {
    p.call("Status", &()).unwrap()
}

fn state(p: &Proxy) -> String {
    let st = status(p);
    <&str>::try_from(&st["state"]).unwrap().to_string()
}

fn search(
    p: &Proxy,
    q: &str,
    limit: u32,
    opts: HashMap<&str, Value<'_>>,
) -> Result<Vec<Hit>, zbus::Error> {
    p.call("Search", &(q, limit, opts))
}

fn wait_state(p: &Proxy, want: &str) {
    let end = Instant::now() + Duration::from_secs(10);
    while Instant::now() < end {
        if state(p) == want {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("state never became {want}: {}", state(p));
}

#[test]
fn cold_start_is_empty_and_scanning_then_ready() {
    let Some(mut b) = bus("cold", 1500, false) else {
        return;
    };
    fs::create_dir_all(b.root.join("docs")).unwrap();
    fs::write(b.root.join("docs/quarterly report.txt"), b"x").unwrap();
    fs::write(b.root.join("Résumé.pdf"), b"x").unwrap();
    fs::write(b.root.join(".hidden-note"), b"x").unwrap();
    fs::write(b.root.join("line\nbreak.txt"), b"x").unwrap();
    let conn = b.conn();
    b.start_service();
    b.wait_for_name(&conn);
    let p = b.proxy(&conn);

    // no snapshot: the name is claimed at once, Search is empty, Status says scanning
    let t0 = Instant::now();
    let hits = search(&p, "report", 10, HashMap::new()).unwrap();
    assert!(hits.is_empty());
    assert!(
        t0.elapsed() < Duration::from_millis(500),
        "a call must not wait on the scan"
    );
    let st = status(&p);
    assert_eq!(<&str>::try_from(&st["state"]).unwrap(), "scanning");
    assert_eq!(<u32>::try_from(&st["entries"]).unwrap(), 0);
    assert_eq!(
        <Vec<String>>::try_from(st["roots"].try_clone().unwrap()).unwrap(),
        vec![format!("file://{}", b.root.display())]
    );

    // StatusChanged arrives when the scan publishes
    let (tx, rx) = mpsc::channel();
    let c2 = b.conn();
    let addr_name = NAME;
    std::thread::spawn(move || {
        let p = Proxy::new(&c2, addr_name, PATH, IFACE).unwrap();
        let mut sigs = p.receive_signal("StatusChanged").unwrap();
        let _ = tx.send(
            sigs.next()
                .map(|m| m.body().deserialize::<HashMap<String, OwnedValue>>().ok()),
        );
    });
    wait_state(&p, "ready");
    let sig = rx
        .recv_timeout(Duration::from_secs(10))
        .expect("StatusChanged was not sent");
    assert!(sig.flatten().is_some());

    // now it answers
    let hits = search(&p, "report", 10, HashMap::new()).unwrap();
    assert_eq!(hits.len(), 1);
    let (uri, name, kind, mime, icon, mtime, size, score) = &hits[0];
    assert_eq!(
        uri,
        &format!("file://{}/docs/quarterly%20report.txt", b.root.display())
    );
    assert_eq!(
        (name.as_str(), kind.as_str(), mime.as_str(), icon.as_str()),
        ("quarterly report.txt", "file", "text/plain", "text-plain")
    );
    assert!(*mtime > 1_500_000_000 && *size == 1 && *score > 0.0);
    // diacritics, display names, hidden files, kinds, root, limit
    assert_eq!(search(&p, "resume", 10, HashMap::new()).unwrap().len(), 1);
    let br = search(&p, "line", 10, HashMap::new()).unwrap();
    assert_eq!(br[0].1, "line\u{240A}break.txt");
    assert!(br[0].0.ends_with("line%0Abreak.txt"));
    assert!(search(&p, "hidden", 10, HashMap::new()).unwrap().is_empty());
    assert_eq!(
        search(
            &p,
            "hidden",
            10,
            HashMap::from([("include_hidden", Value::from(true))])
        )
        .unwrap()
        .len(),
        1
    );
    assert_eq!(
        search(
            &p,
            "docs",
            10,
            HashMap::from([("kind", Value::from("folder"))])
        )
        .unwrap()[0]
            .2,
        "folder"
    );
    assert_eq!(
        search(
            &p,
            "",
            10,
            HashMap::from([("kinds", Value::new(vec!["pdf".to_string()]))])
        )
        .unwrap()
        .len(),
        1
    );
    let root_uri = format!("file://{}/docs", b.root.display());
    assert_eq!(
        search(
            &p,
            "t",
            10,
            HashMap::from([("root", Value::from(root_uri))])
        )
        .unwrap()
        .len(),
        1
    );
    assert_eq!(search(&p, "t", 1, HashMap::new()).unwrap().len(), 1);
    assert!(search(&p, "t", 0, HashMap::new()).unwrap().is_empty());
    // an unknown key is ignored; a wrong type is an error in plain words
    assert!(
        search(
            &p,
            "report",
            10,
            HashMap::from([("future_option", Value::from(1u8))])
        )
        .is_ok()
    );
    match search(
        &p,
        "report",
        10,
        HashMap::from([("size_min", Value::from("big"))]),
    ) {
        Err(zbus::Error::MethodError(name, msg, _)) => {
            assert_eq!(name.as_str(), "org.freedesktop.DBus.Error.InvalidArgs");
            assert!(msg.unwrap_or_default().contains("size_min"));
        }
        other => panic!("expected InvalidArgs, got {other:?}"),
    }

    // NotifyChanged and Refresh over the bus
    fs::write(b.root.join("docs/added-later.txt"), b"x").unwrap();
    let docs = format!("file://{}/docs", b.root.display());
    let _: () = p
        .call("NotifyChanged", &(vec![docs, "http://nope".to_string()],))
        .unwrap();
    let end = Instant::now() + Duration::from_secs(10);
    while search(&p, "added-later", 10, HashMap::new())
        .unwrap()
        .is_empty()
    {
        assert!(Instant::now() < end, "change not seen");
        std::thread::sleep(Duration::from_millis(20));
    }
    let _: () = p.call("Refresh", &()).unwrap();
    wait_state(&p, "ready");
}

#[test]
fn activation_serves_a_restart_from_the_snapshot() {
    let Some(b) = bus("activate", 0, true) else {
        return;
    };
    for i in 0..30 {
        fs::write(b.root.join(format!("file-{i}.txt")), b"x").unwrap();
    }
    let conn = b.conn();
    let p = b.proxy(&conn);
    // the first call starts the service through the bus
    let first = search(&p, "file", 100, HashMap::new()).unwrap();
    assert!(first.len() <= 30);
    wait_state(&p, "ready");
    assert_eq!(search(&p, "file", 100, HashMap::new()).unwrap().len(), 30);
    let pid = b
        .service_pid(&conn)
        .expect("the service runs under the bus");
    // SIGTERM: a pending snapshot is written before exit
    // SAFETY: plain kill(2) on the process the bus started.
    unsafe {
        libc::kill(pid as i32, 15);
    }
    let end = Instant::now() + Duration::from_secs(10);
    while Path::new(&format!("/proc/{pid}")).exists() && !zombie(pid) {
        assert!(Instant::now() < end, "the service did not exit on SIGTERM");
        std::thread::sleep(Duration::from_millis(20));
    }
    let snap = b.dir.join("home/.cache/atlas-explorer/index/v1.idx");
    assert!(snap.exists(), "no snapshot written");
    // the next call activates it again, now with a snapshot to answer from
    fs::write(b.root.join("file-new.txt"), b"x").unwrap();
    let again = search(&p, "file", 100, HashMap::new()).unwrap();
    assert!(
        again.len() >= 30,
        "answered from the snapshot: {}",
        again.len()
    );
}
