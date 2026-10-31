//! `AppHost` for the GPUI app: what core needs from the application.
//!
//! Events go to listeners registered with `listen` (core listens to its own
//! `transcript-update`) and to every UI subscriber. Settings stores are JSON
//! objects in the app data folder, the same files the Tauri store plugin
//! writes. `refresh_status_ui` wakes the tray's status watcher.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};

use serde_json::{Map, Value};
use tingskriver_core::database::manager::DatabaseManager;
use tingskriver_core::host::{AppHost, ListenerId};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

/// An event from core: name and JSON payload.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Event {
    pub name: String,
    pub payload_json: String,
}

type Handler = Arc<dyn Fn(&str) + Send + Sync>;

pub struct Bridge {
    app_data_dir: PathBuf,
    database: RwLock<Option<DatabaseManager>>,
    listeners: Mutex<Vec<(ListenerId, String, Handler)>>,
    next_listener: AtomicU32,
    subscribers: Mutex<Vec<UnboundedSender<Event>>>,
    /// Serialises read-modify-write of the settings files.
    settings: Mutex<()>,
    status_changed: OnceLock<Box<dyn Fn() + Send + Sync>>,
}

impl Bridge {
    pub fn new(app_data_dir: PathBuf) -> Bridge {
        Bridge {
            app_data_dir,
            database: RwLock::new(None),
            listeners: Mutex::new(Vec::new()),
            next_listener: AtomicU32::new(1),
            subscribers: Mutex::new(Vec::new()),
            settings: Mutex::new(()),
            status_changed: OnceLock::new(),
        }
    }

    /// The database once the app has opened it.
    pub fn set_database(&self, database: Option<DatabaseManager>) {
        if let Ok(mut slot) = self.database.write() {
            *slot = database;
        }
    }

    /// Every event core emits from now on.
    pub fn subscribe(&self) -> UnboundedReceiver<Event> {
        let (tx, rx) = unbounded_channel();
        if let Ok(mut subscribers) = self.subscribers.lock() {
            subscribers.push(tx);
        }
        rx
    }

    /// Called on `refresh_status_ui`. Set once, by the host.
    pub(crate) fn on_status_change(&self, f: Box<dyn Fn() + Send + Sync>) {
        let _ = self.status_changed.set(f);
    }

    fn store_path(&self, store: &str) -> Result<PathBuf, String> {
        // A store is a plain file name; never let it point elsewhere.
        if store.is_empty()
            || Path::new(store)
                .file_name()
                .map(|n| n != store)
                .unwrap_or(true)
        {
            return Err(format!("invalid settings store name {store:?}"));
        }
        Ok(self.app_data_dir.join(store))
    }

    fn read_store(path: &Path) -> Result<Map<String, Value>, String> {
        match std::fs::read(path) {
            Ok(bytes) if bytes.iter().all(u8::is_ascii_whitespace) => Ok(Map::new()),
            Ok(bytes) => match serde_json::from_slice::<Value>(&bytes) {
                Ok(Value::Object(map)) => Ok(map),
                Ok(_) => Err(format!("{} is not a JSON object", path.display())),
                Err(e) => Err(format!("{} is not valid JSON: {e}", path.display())),
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Map::new()),
            Err(e) => Err(format!("could not read {}: {e}", path.display())),
        }
    }
}

impl AppHost for Bridge {
    fn emit_json(&self, event: &str, payload_json: &str) -> Result<(), String> {
        // Call handlers outside the lock, so a handler may listen or emit.
        let handlers: Vec<Handler> = match self.listeners.lock() {
            Ok(listeners) => listeners
                .iter()
                .filter(|(_, name, _)| name == event)
                .map(|(_, _, handler)| handler.clone())
                .collect(),
            Err(_) => Vec::new(),
        };
        for handler in handlers {
            handler(payload_json);
        }
        if let Ok(mut subscribers) = self.subscribers.lock() {
            subscribers.retain(|tx| {
                tx.send(Event {
                    name: event.to_string(),
                    payload_json: payload_json.to_string(),
                })
                .is_ok()
            });
        }
        Ok(())
    }

    fn database(&self) -> Option<DatabaseManager> {
        self.database.read().ok().and_then(|db| db.clone())
    }

    fn app_data_dir(&self) -> Result<PathBuf, String> {
        Ok(self.app_data_dir.clone())
    }

    fn listen(&self, event: &str, handler: Box<dyn Fn(&str) + Send + Sync>) -> ListenerId {
        let id = self.next_listener.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut listeners) = self.listeners.lock() {
            listeners.push((id, event.to_string(), Arc::from(handler)));
        }
        id
    }

    fn unlisten(&self, id: ListenerId) {
        if let Ok(mut listeners) = self.listeners.lock() {
            listeners.retain(|(listener, _, _)| *listener != id);
        }
    }

    fn read_setting(&self, store: &str, key: &str) -> Result<Option<Value>, String> {
        let path = self.store_path(store)?;
        let _guard = self.settings.lock().map_err(|_| "settings lock poisoned")?;
        Ok(Self::read_store(&path)?.get(key).cloned())
    }

    fn write_setting(&self, store: &str, key: &str, value: Value) -> Result<(), String> {
        let path = self.store_path(store)?;
        let _guard = self.settings.lock().map_err(|_| "settings lock poisoned")?;
        let mut map = Self::read_store(&path)?;
        map.insert(key.to_string(), value);
        let text = serde_json::to_string_pretty(&Value::Object(map)).map_err(|e| e.to_string())?;
        std::fs::create_dir_all(&self.app_data_dir)
            .map_err(|e| format!("could not create {}: {e}", self.app_data_dir.display()))?;
        // Write next to the file and rename, so a crash never leaves half a file.
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, text)
            .map_err(|e| format!("could not write {}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, &path)
            .map_err(|e| format!("could not replace {}: {e}", path.display()))
    }

    fn refresh_status_ui(&self) {
        if let Some(f) = self.status_changed.get() {
            f();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use tingskriver_core::host::{HostExt, HostHandle};

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("tingskriver-host-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn listeners_get_their_events_until_unlisten() {
        let bridge = Bridge::new(temp_dir("listen"));
        let seen = Arc::new(Mutex::new(Vec::new()));
        let s = seen.clone();
        let id = bridge.listen(
            "transcript-update",
            Box::new(move |p| s.lock().unwrap().push(p.to_string())),
        );
        bridge
            .emit_json("transcript-update", r#"{"text":"a"}"#)
            .unwrap();
        bridge.emit_json("other", "1").unwrap();
        bridge.unlisten(id);
        bridge
            .emit_json("transcript-update", r#"{"text":"b"}"#)
            .unwrap();
        assert_eq!(*seen.lock().unwrap(), vec![r#"{"text":"a"}"#.to_string()]);
    }

    #[test]
    fn a_handler_may_emit_again() {
        let bridge = Arc::new(Bridge::new(temp_dir("reentrant")));
        let count = Arc::new(AtomicUsize::new(0));
        let (b, c) = (bridge.clone(), count.clone());
        bridge.listen(
            "ping",
            Box::new(move |_| {
                c.fetch_add(1, Ordering::SeqCst);
                b.emit_json("pong", "null").unwrap();
            }),
        );
        let mut events = bridge.subscribe();
        bridge.emit_json("ping", "null").unwrap();
        assert_eq!(count.load(Ordering::SeqCst), 1);
        assert_eq!(events.try_recv().unwrap().name, "pong");
        assert_eq!(events.try_recv().unwrap().name, "ping");
    }

    #[test]
    fn subscribers_get_every_event_and_closed_ones_are_dropped() {
        let bridge = Bridge::new(temp_dir("subscribe"));
        let mut a = bridge.subscribe();
        let b = bridge.subscribe();
        drop(b);
        let handle: HostHandle = Arc::new(bridge);
        handle
            .emit("recording-started", serde_json::json!({"workers": 3}))
            .unwrap();
        assert_eq!(
            a.try_recv().unwrap(),
            Event {
                name: "recording-started".into(),
                payload_json: r#"{"workers":3}"#.into()
            }
        );
    }

    #[test]
    fn settings_persist_per_store_and_key() {
        let dir = temp_dir("settings");
        let bridge = Bridge::new(dir.clone());
        assert_eq!(
            bridge.read_setting("shortcuts.json", "shortcuts").unwrap(),
            None
        );
        bridge
            .write_setting(
                "shortcuts.json",
                "shortcuts",
                serde_json::json!({"ask": "Ctrl+Alt+Space"}),
            )
            .unwrap();
        bridge
            .write_setting("shortcuts.json", "other", serde_json::json!(1))
            .unwrap();
        // A new bridge reads the same file.
        let again = Bridge::new(dir.clone());
        assert_eq!(
            again.read_setting("shortcuts.json", "shortcuts").unwrap(),
            Some(serde_json::json!({"ask": "Ctrl+Alt+Space"}))
        );
        assert_eq!(
            again.read_setting("shortcuts.json", "other").unwrap(),
            Some(serde_json::json!(1))
        );
        assert!(!dir.join("shortcuts.json.tmp").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn broken_or_foreign_store_names_are_errors() {
        let dir = temp_dir("broken");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("bad.json"), "{nope").unwrap();
        let bridge = Bridge::new(dir.clone());
        assert!(bridge.read_setting("bad.json", "x").is_err());
        // A broken file is not overwritten with an empty object.
        assert!(bridge.write_setting("bad.json", "x", Value::Null).is_err());
        assert_eq!(
            std::fs::read_to_string(dir.join("bad.json")).unwrap(),
            "{nope"
        );
        assert!(bridge.read_setting("../evil.json", "x").is_err());
        assert!(bridge.write_setting("", "x", Value::Null).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn refresh_status_ui_calls_the_host() {
        let bridge = Bridge::new(temp_dir("refresh"));
        bridge.refresh_status_ui();
        let count = Arc::new(AtomicUsize::new(0));
        let c = count.clone();
        bridge.on_status_change(Box::new(move || {
            c.fetch_add(1, Ordering::SeqCst);
        }));
        bridge.refresh_status_ui();
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }
}
