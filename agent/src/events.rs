//! Telefonlara yayınlanan durum olayları (medya, ses, zamanlayıcı, öndeki uygulama).
//! Her türün son hali saklanır: yeni bağlanan telefon hepsini hemen alır.
use crate::sync::LockExt;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;

pub struct Events {
    tx: broadcast::Sender<String>,
    latest: Mutex<HashMap<&'static str, Value>>,
}

impl Events {
    pub fn new() -> Arc<Events> {
        let (tx, _) = broadcast::channel(64);
        Arc::new(Events { tx, latest: Mutex::new(HashMap::new()) })
    }

    /// Olayı yayınla ve son hali olarak sakla
    pub fn publish(&self, kind: &'static str, value: Value) {
        self.publish_split(kind, value.clone(), value);
    }

    /// Saklanan hal ile gönderilen farklı olabilir (ör. medya: kapak görseli sadece değişince gönderilir)
    pub fn publish_split(&self, kind: &'static str, mut keep: Value, mut send: Value) {
        keep["type"] = json!(kind);
        send["type"] = json!(kind);
        self.latest.lk().insert(kind, keep);
        let _ = self.tx.send(send.to_string());
    }

    pub fn latest(&self, kind: &str) -> Option<Value> {
        self.latest.lk().get(kind).cloned()
    }

    pub fn subscribe(&self) -> broadcast::Receiver<String> {
        self.tx.subscribe()
    }

    /// Yeni bağlanan telefona gönderilecek son durumlar
    pub fn snapshot(&self) -> Vec<String> {
        self.latest.lk().values().map(Value::to_string).collect()
    }
}
