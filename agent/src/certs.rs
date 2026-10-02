//! Yerel sertifika otoritesi (yedek yöntem). CA'ya ad kısıtlaması konur: sadece kendi .local adımız
//! için sertifika imzalayabilir; anahtar çalınsa bile başka siteleri taklit etmekte kullanılamaz.
use anyhow::Result;
use rcgen::{
    BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, GeneralSubtree, IsCa, Issuer,
    KeyPair, KeyUsagePurpose, NameConstraints, PKCS_ECDSA_P256_SHA256,
};
use time::{Duration, OffsetDateTime};

use crate::dpapi;
use crate::state::StoredCert;

const LEAF_DAYS: i64 = 397; // iOS: TLS sertifikası 398 günden uzun olamaz
pub const RENEW_BEFORE_DAYS: i64 = 30;

fn rfc3339(t: OffsetDateTime) -> String {
    t.format(&time::format_description::well_known::Rfc3339).unwrap_or_default()
}

pub fn expires_soon(cert: &StoredCert) -> bool {
    OffsetDateTime::parse(&cert.not_after, &time::format_description::well_known::Rfc3339)
        .map(|t| t - OffsetDateTime::now_utc() < Duration::days(RENEW_BEFORE_DAYS))
        .unwrap_or(true)
}

pub fn create_ca(host: &str) -> Result<StoredCert> {
    let key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256)?;
    let mut p = CertificateParams::new(Vec::<String>::new())?;
    p.distinguished_name.push(DnType::CommonName, format!("CouchRemote Yerel CA ({host})"));
    p.distinguished_name.push(DnType::OrganizationName, "CouchRemote");
    p.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));
    p.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
    p.name_constraints = Some(NameConstraints {
        permitted_subtrees: vec![GeneralSubtree::DnsName(host.to_string())],
        excluded_subtrees: vec![],
    });
    let now = OffsetDateTime::now_utc();
    p.not_before = now - Duration::days(1);
    p.not_after = now + Duration::days(3650);
    let cert = p.self_signed(&key)?;
    Ok(StoredCert {
        host: host.to_string(),
        cert_pem: cert.pem(),
        key_pem_enc: dpapi::protect_str(&key.serialize_pem())?,
        not_after: rfc3339(p.not_after),
        ip: String::new(),
    })
}

pub fn create_leaf(host: &str, lan_ip: &str, ca: &StoredCert) -> Result<StoredCert> {
    let ca_key = KeyPair::from_pem(&dpapi::unprotect_str(&ca.key_pem_enc)?)?;
    let issuer = Issuer::from_ca_cert_pem(&ca.cert_pem, ca_key)?;
    let key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256)?;
    let mut p = CertificateParams::new(vec![host.to_string()])?;
    // .local adı Android'de çözülemezse LAN adresiyle de açılabilsin (CA'nın ad kısıtı yalnızca DNS adlarını sınırlar)
    if let Ok(ip) = lan_ip.parse::<std::net::IpAddr>() {
        p.subject_alt_names.push(rcgen::SanType::IpAddress(ip));
    }
    p.distinguished_name.push(DnType::CommonName, host);
    p.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    p.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    p.use_authority_key_identifier_extension = true;
    let now = OffsetDateTime::now_utc();
    p.not_before = now - Duration::days(1);
    p.not_after = now + Duration::days(LEAF_DAYS);
    let cert = p.signed_by(&key, &issuer)?;
    Ok(StoredCert {
        host: host.to_string(),
        cert_pem: format!("{}\n{}", cert.pem().trim(), ca.cert_pem.trim()),
        key_pem_enc: dpapi::protect_str(&key.serialize_pem())?,
        not_after: rfc3339(p.not_after),
        ip: lan_ip.to_string(),
    })
}

/// Sertifikanın kısa doğrulama kodu (SHA-256'nın ilk 4 baytı), ör. "7F3A-91C2".
/// iPhone'da profil adında görünür; kullanıcı PC ekranındakiyle karşılaştırır (kurulum sayfası HTTP olduğu
/// için araya giren biri başka bir profil verirse kod tutmaz — S3).
pub fn fingerprint_code(der: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let d = Sha256::digest(der);
    format!("{:02X}{:02X}-{:02X}{:02X}", d[0], d[1], d[2], d[3])
}

pub fn ca_der(ca: &StoredCert) -> Result<Vec<u8>> {
    use rustls::pki_types::{pem::PemObject, CertificateDer};
    Ok(CertificateDer::from_pem_slice(ca.cert_pem.as_bytes())?.to_vec())
}

/// iPhone'a yüklenecek yapılandırma profili (sadece kök sertifika içerir).
pub fn mobile_config(host: &str, id: &str, outer: &str, inner: &str, ca_der: &[u8]) -> String {
    use base64::{engine::general_purpose::STANDARD, Engine};
    let data = STANDARD.encode(ca_der);
    let code = fingerprint_code(ca_der);
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>PayloadContent</key>
  <array>
    <dict>
      <key>PayloadCertificateFileName</key><string>couchremote-ca-{id}.cer</string>
      <key>PayloadContent</key><data>{data}</data>
      <key>PayloadDescription</key><string>CouchRemote'un {host} adresine güvenli bağlanabilmesi için yerel sertifika.</string>
      <key>PayloadDisplayName</key><string>CouchRemote Yerel CA · {code}</string>
      <key>PayloadIdentifier</key><string>app.couchremote.ca.{inner}</string>
      <key>PayloadType</key><string>com.apple.security.root</string>
      <key>PayloadUUID</key><string>{inner}</string>
      <key>PayloadVersion</key><integer>1</integer>
    </dict>
  </array>
  <key>PayloadDescription</key><string>Sadece {host} için geçerlidir (ad kısıtlamalı). Doğrulama kodu {code} bilgisayar ekranındakiyle aynı olmalı.</string>
  <key>PayloadDisplayName</key><string>CouchRemote · {code}</string>
  <key>PayloadIdentifier</key><string>app.couchremote.profile.{outer}</string>
  <key>PayloadRemovalDisallowed</key><false/>
  <key>PayloadType</key><string>Configuration</string>
  <key>PayloadUUID</key><string>{outer}</string>
  <key>PayloadVersion</key><integer>1</integer>
</dict>
</plist>
"#
    )
}
