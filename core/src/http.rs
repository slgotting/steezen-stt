use reqwest::blocking::ClientBuilder;

#[cfg(not(target_os = "android"))]
pub fn builder() -> ClientBuilder {
    reqwest::blocking::Client::builder()
}

#[cfg(target_os = "android")]
pub fn builder() -> ClientBuilder {
    let roots = rustls::RootCertStore { roots: webpki_roots::TLS_SERVER_ROOTS.to_vec() };
    let config = rustls::ClientConfig::builder_with_provider(std::sync::Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .expect("TLS versions")
        .with_root_certificates(roots)
        .with_no_client_auth();
    reqwest::blocking::Client::builder().tls_backend_preconfigured(config)
}

pub fn client() -> reqwest::blocking::Client {
    builder().connect_timeout(std::time::Duration::from_secs(10)).build().unwrap_or_default()
}
