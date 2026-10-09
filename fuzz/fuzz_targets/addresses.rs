#![no_main]
//! Everything that turns text into an address to open: launch arguments (the
//! command line, D-Bus), the address bar, Connect to Server, the recent list.
use atlas_explorer_core::{address, launch, location, servers};
use libfuzzer_sys::fuzz_target;
use std::path::Path;

fn no_password(url: &str) -> bool {
    // scheme://user:password@host: the userinfo of a server address may not
    // hold a colon (the other schemes have no user or password)
    match url.split_once("://") {
        Some((scheme, rest)) if launch::SERVER_SCHEMES.contains(&scheme.to_ascii_lowercase().as_str()) => {
            let auth = &rest[..rest.find(['/', '?', '#']).unwrap_or(rest.len())];
            match auth.rsplit_once('@') {
                Some((user, _)) => !user.contains(':'),
                None => true,
            }
        }
        _ => true,
    }
}

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data).into_owned();
    let args: Vec<String> = text.split('\n').map(str::to_string).collect();
    for from_bus in [false, true] {
        let mut a = args.clone();
        if from_bus {
            a.insert(0, "--bus".into());
        }
        let l = launch::parse(&a, Path::new("/home/u"));
        assert!(l.locations.len() <= launch::MAX_ARGS);
        for loc in &l.locations {
            assert!(!loc.chars().any(launch::is_hidden_char), "{loc:?}");
            assert!(no_password(loc), "{loc:?}");
            if from_bus {
                let scheme = loc.split(':').next().unwrap_or("");
                assert!(!launch::SERVER_SCHEMES.contains(&scheme.to_ascii_lowercase().as_str()));
            }
        }
        for r in &l.refused {
            assert!(no_password(&r.arg), "{:?}", r.arg);
        }
    }
    if let Ok(url) = address::parse(&text, "file:///home/u", Path::new("/home/u")) {
        assert!(no_password(&url), "{url:?}");
        assert!(!url.chars().any(launch::is_hidden_char));
        let _ = location::segments(&url, "/home/u");
    }
    let _ = address::split_for_completion(&text, "file:///home/u", Path::new("/home/u"));
    let _ = address::url_to_path(&text);
    // Connect to Server: the fields of a form, then the address as it is read back
    let mut f = text.splitn(4, '\n');
    let (server, folder, user) = (
        f.next().unwrap_or(""),
        f.next().unwrap_or(""),
        f.next().unwrap_or(""),
    );
    for p in servers::PROTOCOLS {
        if let Ok(url) = servers::build(p, server, folder, user) {
            assert!(no_password(&url), "{url:?}");
            assert!(url.len() <= servers::MAX_URL + 1024);
            let parts = servers::parse_url(&url).expect("an address it built is one it reads");
            let again = servers::build(parts.protocol, &parts.server, &parts.folder, &parts.user)
                .expect("and builds again");
            assert_eq!(again, url);
        }
    }
    if let Some(url) = servers::clean_recent(&text) {
        assert!(no_password(&url));
        assert_eq!(servers::clean_recent(&url).as_deref(), Some(url.as_str()));
    }
    let r = servers::Recents::parse(&text);
    assert!(r.urls().len() <= servers::MAX_RECENT);
    let _ = servers::recent_label(&text);
    let _ = servers::security_note(&text);
});
