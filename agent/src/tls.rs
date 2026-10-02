//! HTTPS sunucusu: her bağlantıda TCP_NODELAY açılır, sertifika SNI'a (istenen ada) göre seçilir.
//! Sertifikalar çalışırken değiştirilebilir (yenileme yeniden başlatma gerektirmez).
use anyhow::Result;
use rustls::pki_types::{pem::PemObject, CertificateDer, PrivateKeyDer};
use rustls::server::{ClientHello, ResolvesServerCert};
use rustls::sign::CertifiedKey;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// Bağlantının karşı tarafının IP'si (istek uzantısı olarak işleyicilere verilir)
#[derive(Clone, Copy, Debug)]
pub struct Peer(pub std::net::IpAddr);

#[derive(Debug, Default)]
pub struct SniResolver {
    certs: RwLock<HashMap<String, Arc<CertifiedKey>>>,
    /// SNI göndermeyen istemciler (IP adresiyle bağlananlar) için tek sertifika
    fallback: RwLock<Option<Arc<CertifiedKey>>>,
}

impl SniResolver {
    pub fn set(&self, host: &str, cert_pem: &str, key_pem: &str) -> Result<()> {
        let chain = CertificateDer::pem_slice_iter(cert_pem.as_bytes()).collect::<Result<Vec<_>, _>>()?;
        let key = PrivateKeyDer::from_pem_slice(key_pem.as_bytes())?;
        let signer = rustls::crypto::ring::sign::any_supported_type(&key)?;
        let ck = Arc::new(CertifiedKey::new(chain, signer));
        self.certs.write().unwrap().insert(host.to_ascii_lowercase(), ck.clone());
        *self.fallback.write().unwrap() = Some(ck);
        Ok(())
    }

}

impl ResolvesServerCert for SniResolver {
    fn resolve(&self, hello: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        match hello.server_name() {
            Some(name) => self.certs.read().unwrap().get(&name.to_ascii_lowercase()).cloned(),
            None => self.fallback.read().unwrap().clone(),
        }
    }
}

pub fn server_config(resolver: Arc<SniResolver>) -> Arc<rustls::ServerConfig> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut cfg = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .expect("TLS sürümleri")
        .with_no_client_auth()
        .with_cert_resolver(resolver);
    // WebSocket'i basit tutmak için sadece HTTP/1.1
    cfg.alpn_protocols = vec![b"http/1.1".to_vec()];
    Arc::new(cfg)
}
