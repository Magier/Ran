use std::fs;
use std::io::BufReader;
use std::net::TcpStream;
use std::path::Path;
use std::sync::Arc;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, DigitallySignedStruct, RootCertStore, SignatureScheme};
use serde_json::{json, Value};
use tungstenite::client::IntoClientRequest;
use tungstenite::{protocol::Message, Connector};
use url::Url;

pub(crate) fn run(
    raw_url: &str,
    token_file: Option<&Path>,
    ca_file: Option<&Path>,
    insecure_skip_tls_verify: bool,
) -> Result<(), String> {
    let token = load_token(token_file)?;
    let url = Url::parse(raw_url).map_err(|error| format!("invalid URL: {error}"))?;
    let host = url
        .host_str()
        .ok_or_else(|| "URL has no host".to_string())?;
    let port = url
        .port_or_known_default()
        .ok_or_else(|| "URL has no port and uses an unknown scheme".to_string())?;
    let tcp = TcpStream::connect((host, port))
        .map_err(|error| format!("TCP connection failed: {error}"))?;

    let mut request = raw_url
        .into_client_request()
        .map_err(|error| format!("failed to create WebSocket request: {error}"))?;
    request.headers_mut().insert(
        "Sec-WebSocket-Protocol",
        "v4.channel.k8s.io"
            .parse()
            .map_err(|error| format!("invalid WebSocket protocol header: {error}"))?,
    );
    request.headers_mut().insert(
        "Authorization",
        format!("Bearer {token}")
            .parse()
            .map_err(|error| format!("invalid bearer token header: {error}"))?,
    );

    let mut roots = RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    if let Some(path) = ca_file {
        let file = fs::File::open(path).map_err(|error| {
            format!(
                "failed to read CA certificate '{}': {error}",
                path.display()
            )
        })?;
        for certificate in rustls_pemfile::certs(&mut BufReader::new(file)) {
            roots
                .add(
                    certificate
                        .map_err(|error| format!("failed to parse CA certificate: {error}"))?,
                )
                .map_err(|error| format!("failed to trust CA certificate: {error}"))?;
        }
    }
    let mut tls =
        ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .map_err(|error| format!("failed to configure TLS versions: {error}"))?
            .with_root_certificates(roots)
            .with_no_client_auth();
    if insecure_skip_tls_verify {
        tls.dangerous()
            .set_certificate_verifier(Arc::new(InsecureServerVerifier));
    }
    let connector = Connector::Rustls(Arc::new(tls));
    let (mut websocket, _) =
        tungstenite::client_tls_with_config(request, tcp, None, Some(connector))
            .map_err(|error| format!("WebSocket connection failed: {error}"))?;

    let mut result_parts = Vec::new();
    let mut status_data: Option<Value> = None;
    loop {
        match websocket.read() {
            Ok(Message::Binary(data)) if !data.is_empty() => match data[0] {
                0x01 | 0x02 => {
                    let payload = &data[1..];
                    let line = payload.strip_suffix(b"\n").unwrap_or(payload);
                    result_parts.push(String::from_utf8_lossy(line).into_owned());
                }
                0x03 => {
                    if let Ok(value) = serde_json::from_slice::<Value>(&data[1..]) {
                        status_data = Some(value);
                    }
                }
                _ => {}
            },
            Ok(Message::Close(_)) => break,
            Err(error) => return Err(format!("WebSocket read failed: {error}")),
            _ => {}
        }
    }

    let mut output = json!({ "result": result_parts.join("\n").trim() });
    if let (Some(object), Some(status)) = (
        output.as_object_mut(),
        status_data.and_then(|v| v.as_object().cloned()),
    ) {
        object.extend(status);
    }
    println!("{output}");
    Ok(())
}

#[derive(Debug)]
struct InsecureServerVerifier;

impl ServerCertVerifier for InsecureServerVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _certificate: &CertificateDer<'_>,
        _signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _certificate: &CertificateDer<'_>,
        _signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![
            SignatureScheme::ECDSA_NISTP256_SHA256,
            SignatureScheme::ECDSA_NISTP384_SHA384,
            SignatureScheme::ED25519,
            SignatureScheme::RSA_PSS_SHA256,
            SignatureScheme::RSA_PSS_SHA384,
            SignatureScheme::RSA_PSS_SHA512,
            SignatureScheme::RSA_PKCS1_SHA256,
            SignatureScheme::RSA_PKCS1_SHA384,
            SignatureScheme::RSA_PKCS1_SHA512,
        ]
    }
}

fn load_token(token_file: Option<&Path>) -> Result<String, String> {
    let value = if let Some(path) = token_file {
        fs::read_to_string(path)
            .map_err(|error| format!("failed to read token file '{}': {error}", path.display()))?
    } else if let Ok(token) = std::env::var("RANPLANT_TOKEN") {
        std::env::remove_var("RANPLANT_TOKEN");
        token
    } else if let Ok(token) = std::env::var("TOKEN") {
        std::env::remove_var("TOKEN");
        token
    } else {
        return Err("no bearer token provided; use --token-file or RANPLANT_TOKEN".to_string());
    };
    let token = value.trim().to_string();
    if token.is_empty() {
        return Err("bearer token is empty".to_string());
    }
    Ok(token)
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::load_token;

    #[test]
    fn token_is_not_accepted_as_a_command_line_argument() {
        let command = crate::Cli::try_parse_from([
            "ranplant",
            "kubelet-exec",
            "--url",
            "wss://example.invalid/exec",
            "--token",
            "secret",
        ]);
        assert!(command.is_err());
    }

    #[test]
    fn rejects_an_empty_environment_token() {
        std::env::set_var("RANPLANT_TOKEN", "  ");
        let error = load_token(None).expect_err("empty token must fail");
        assert_eq!(error, "bearer token is empty");
    }
}
