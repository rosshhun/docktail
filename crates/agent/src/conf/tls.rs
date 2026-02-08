//! TLS — rustls ServerConfig building for mTLS.

use std::sync::Arc;
use std::fs::File;
use std::io::BufReader;
use rustls::ServerConfig;
use rustls::pki_types::CertificateDer;

use super::model::AgentConfig;

impl AgentConfig {
    /// Build a rustls ServerConfig with mTLS from the configuration
    pub fn build_rustls_config(&self) -> Result<Arc<ServerConfig>, Box<dyn std::error::Error>> {
        // Load certificates
        let cert_file = File::open(&self.tls_cert_path)?;
        let mut cert_reader = BufReader::new(cert_file);
        let certs: Vec<CertificateDer> = rustls_pemfile::certs(&mut cert_reader)
            .collect::<Result<Vec<_>, _>>()?;

        // Load private key
        let key_file = File::open(&self.tls_key_path)?;
        let mut key_reader = BufReader::new(key_file);
        let key = rustls_pemfile::private_key(&mut key_reader)?
            .ok_or("No private key found in file")?;

        // Load CA certificate for client verification (mTLS)
        let ca_file = File::open(&self.tls_ca_path)?;
        let mut ca_reader = BufReader::new(ca_file);
        let ca_certs: Vec<CertificateDer> = rustls_pemfile::certs(&mut ca_reader)
            .collect::<Result<Vec<_>, _>>()?;

        // Create client certificate verifier
        let mut root_store = rustls::RootCertStore::empty();
        for cert in ca_certs {
            root_store.add(cert)?;
        }
        
        let client_verifier = rustls::server::WebPkiClientVerifier::builder(
            Arc::new(root_store)
        ).build()?;

        // Build server config with mTLS
        let mut config = ServerConfig::builder()
            .with_client_cert_verifier(client_verifier)
            .with_single_cert(certs, key)?;

        // Configure ALPN to support HTTP/2 (required for gRPC)
        config.alpn_protocols = vec![b"h2".to_vec()];

        Ok(Arc::new(config))
    }
}
