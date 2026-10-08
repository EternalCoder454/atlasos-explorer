//! The D-Bus object: `net.eterneon.telamon.explorer.Search1`, and for this
//! release the same object under the name the interface had before the
//! rename, `net.eterneon.atlas.explorer.Search1` (the Launcher still calls it).

use crate::options;
use atlas_explorer_core::display_name;
use atlas_file_index::uri::path_to_uri;
use atlas_file_index::{Engine, Status};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};
use zbus::fdo;
use zbus::object_server::SignalEmitter;
use zbus::zvariant::{OwnedValue, Value};

/// `(uri, name, kind, mime, icon, mtime, size, score)`
pub type HitTuple = (String, String, String, String, String, i64, u64, f64);

/// Searches running at once; more are refused, so a process that floods the
/// service cannot make it start threads without limit.
const MAX_SEARCHES: usize = 8;

#[derive(Clone)]
pub struct Search1 {
    engine: Arc<Engine>,
    running: Arc<AtomicUsize>,
}

/// One running search; frees its place when dropped.
struct Slot1(Arc<AtomicUsize>);

impl Drop for Slot1 {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

impl Search1 {
    pub fn new(engine: Arc<Engine>) -> Self {
        Search1 {
            engine,
            running: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn take_place(&self) -> Option<Slot1> {
        let n = self.running.fetch_add(1, Ordering::AcqRel);
        let slot = Slot1(self.running.clone());
        (n < MAX_SEARCHES).then_some(slot)
    }
}

/// The reply of `Status`, and the body of `StatusChanged`.
pub fn status_map(s: &Status) -> HashMap<String, OwnedValue> {
    let mut m: HashMap<String, OwnedValue> = HashMap::new();
    let mut put = |k: &str, v: Value<'_>| {
        if let Ok(o) = OwnedValue::try_from(v) {
            m.insert(k.to_string(), o);
        }
    };
    put("state", Value::from(s.state.as_str()));
    put("entries", Value::from(s.entries));
    put("updated", Value::from(s.updated));
    put("roots", Value::new(s.roots.clone()));
    if !s.error.is_empty() {
        // plain words, never a name from the file system: but cap it anyway
        put("error", Value::from(display_name(s.error.as_str())));
    }
    m
}

/// A result handed back from another thread to an `async` method.
struct Slot<T> {
    value: Option<Result<T, ()>>,
    waker: Option<Waker>,
}

struct Waiting<T>(Arc<Mutex<Slot<T>>>);

impl<T> Future for Waiting<T> {
    type Output = Result<T, ()>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut s = self.0.lock().unwrap_or_else(|e| e.into_inner());
        match s.value.take() {
            Some(v) => Poll::Ready(v),
            None => {
                s.waker = Some(cx.waker().clone());
                Poll::Pending
            }
        }
    }
}

/// Run `f` on its own thread (one thread per call, so rapid calls never queue
/// behind each other or block the bus executor) and wait for it without
/// blocking. `Err` if the thread could not start or `f` panicked.
fn on_own_thread<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Waiting<T> {
    let slot = Arc::new(Mutex::new(Slot {
        value: None,
        waker: None,
    }));
    let s2 = slot.clone();
    let spawned = std::thread::Builder::new()
        .name("search".into())
        .spawn(move || {
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).map_err(|_| ());
            let mut s = s2.lock().unwrap_or_else(|e| e.into_inner());
            s.value = Some(r);
            if let Some(w) = s.waker.take() {
                w.wake();
            }
        });
    if spawned.is_err() {
        slot.lock().unwrap_or_else(|e| e.into_inner()).value = Some(Err(()));
    }
    Waiting(slot)
}

impl Search1 {
    /// The tags in use: `(name, entries)`, most used first, at most 500.
    async fn do_tags(&self) -> fdo::Result<Vec<(String, u32)>> {
        let Some(place) = self.take_place() else {
            return Err(fdo::Error::LimitsExceeded(
                "too many searches are running, try again in a moment".into(),
            ));
        };
        let engine = self.engine.clone();
        on_own_thread(move || {
            let _place = place;
            engine.tags()
        })
        .await
        .map_err(|()| fdo::Error::Failed("the service failed inside; see its log".into()))
    }

    /// Search the index. At most `limit` hits (cap 500), best first.
    async fn do_search(
        &self,
        query: String,
        limit: u32,
        options: HashMap<String, OwnedValue>,
    ) -> fdo::Result<Vec<HitTuple>> {
        let opts = options::parse(&options).map_err(fdo::Error::InvalidArgs)?;
        let Some(place) = self.take_place() else {
            return Err(fdo::Error::LimitsExceeded(
                "too many searches are running, try again in a moment".into(),
            ));
        };
        let engine = self.engine.clone();
        let hits = on_own_thread(move || {
            let _place = place;
            engine.search(&query, limit as usize, &opts)
        })
        .await
        .map_err(|()| {
            fdo::Error::Failed("the search failed inside the service; see its log".into())
        })?;
        Ok(hits
            .into_iter()
            .map(|h| {
                let mime = h.mime();
                let icon = h.icon();
                (
                    path_to_uri(&h.path),
                    h.name,
                    if h.is_dir { "folder" } else { "file" }.to_string(),
                    mime.to_string(),
                    icon,
                    h.mtime,
                    h.size,
                    h.score,
                )
            })
            .collect())
    }
}

/// The interface, as `net.eterneon.telamon.explorer.Search1`.
#[zbus::interface(name = "net.eterneon.telamon.explorer.Search1")]
impl Search1 {
    /// Search the index. At most `limit` hits (cap 500), best first.
    async fn search(
        &self,
        query: String,
        limit: u32,
        options: HashMap<String, OwnedValue>,
    ) -> fdo::Result<Vec<HitTuple>> {
        self.do_search(query, limit, options).await
    }

    /// The tags in use (`user.xdg.tags`): name and how many files and folders
    /// carry it, most used first, at most 500. Names that differ only in case
    /// are one tag.
    async fn tags(&self) -> fdo::Result<Vec<(String, u32)>> {
        self.do_tags().await
    }

    /// State of the index.
    async fn status(&self) -> HashMap<String, OwnedValue> {
        status_map(&self.engine.status())
    }

    /// Hint from Explorer after its own operations: rescan these folders.
    /// Only `file://` URIs are used, at most 256.
    async fn notify_changed(&self, uris: Vec<String>) {
        self.engine.notify_changed(&uris);
    }

    /// Rescan everything now.
    async fn refresh(&self) {
        self.engine.refresh();
    }

    #[zbus(signal)]
    async fn status_changed(
        emitter: &SignalEmitter<'_>,
        status: HashMap<String, OwnedValue>,
    ) -> zbus::Result<()>;
}

/// The same interface under its name before the rename
/// (`net.eterneon.atlas.explorer.Search1`, at `/net/eterneon/atlas/explorer/Search`),
/// for the apps that have not moved yet. Same engine, same limit on running
/// searches. Remove with the old bus name in the release after.
pub struct LegacySearch1(pub Search1);

#[zbus::interface(name = "net.eterneon.atlas.explorer.Search1")]
impl LegacySearch1 {
    async fn search(
        &self,
        query: String,
        limit: u32,
        options: HashMap<String, OwnedValue>,
    ) -> fdo::Result<Vec<HitTuple>> {
        self.0.do_search(query, limit, options).await
    }

    async fn tags(&self) -> fdo::Result<Vec<(String, u32)>> {
        self.0.do_tags().await
    }

    async fn status(&self) -> HashMap<String, OwnedValue> {
        status_map(&self.0.engine.status())
    }

    async fn notify_changed(&self, uris: Vec<String>) {
        self.0.engine.notify_changed(&uris);
    }

    async fn refresh(&self) {
        self.0.engine.refresh();
    }

    #[zbus(signal)]
    async fn status_changed(
        emitter: &SignalEmitter<'_>,
        status: HashMap<String, OwnedValue>,
    ) -> zbus::Result<()>;
}
