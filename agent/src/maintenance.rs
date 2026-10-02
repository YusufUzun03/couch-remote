//! Bakım döngüsü: yerel sunucu sertifikası bitmeden yenilenir (12 saatte bir kontrol).
use crate::sync::LockExt;
use std::sync::Arc;
use std::time::Duration;

use crate::app::App;
use crate::{certs, dpapi};

pub async fn run(app: Arc<App>) {
    loop {
        renew_local_leaf(&app);
        crate::state::snapshot_backup();
        tokio::time::sleep(Duration::from_secs(12 * 3600)).await;
    }
}

fn renew_local_leaf(app: &App) {
    let mut st = app.state.lk();
    let (Some(ca), Some(leaf)) = (st.ca.clone(), st.leaf.as_ref()) else { return };
    if !certs::expires_soon(leaf) && leaf.ip == app.lan_ip.to_string() {
        return;
    }
    match certs::create_leaf(&app.host, &app.lan_ip.to_string(), &ca).and_then(|l| {
        app.resolver.set(&l.host, &l.cert_pem, &dpapi::unprotect_str(&l.key_pem_enc)?)?;
        Ok(l)
    }) {
        Ok(l) => {
            st.leaf = Some(l);
            let _ = st.save();
            log!("Yerel sertifika yenilendi");
        }
        Err(e) => log!("Yerel sertifika yenilenemedi: {e:#}"),
    }
}
