//! TLS for the game's STARTTLS. The game's TLS stack is old: SBRW ran
//! Openfire on JDK 10 with `-Djsse.enableCBCProtection=false`, and the
//! reference container settles on TLS 1.0 with `ECDHE-RSA-AES256-SHA`
//! (`docs/protocol/xmpp.md`). So the acceptor allows TLS 1.0 and every cipher
//! OpenSSL still has, and doesn't split records. Whether the real client
//! completes this handshake is unverified.

use openssl::asn1::Asn1Time;
use openssl::bn::{BigNum, MsbOption};
use openssl::error::ErrorStack;
use openssl::hash::MessageDigest;
use openssl::pkey::{PKey, Private};
use openssl::rsa::Rsa;
use openssl::ssl::{SslAcceptor, SslMethod, SslOptions, SslVersion};
use openssl::x509::{X509, X509NameBuilder};

/// Openfire's cipher choice reached with nothing configured.
pub const DEFAULT_CIPHERS: &str = "DEFAULT:@SECLEVEL=0";

pub struct Identity {
    pub certificate: X509,
    pub key: PKey<Private>,
}

/// A self-signed RSA-2048 certificate for `host`, valid for ten years.
pub fn self_signed(host: &str) -> Result<Identity, ErrorStack> {
    let key = PKey::from_rsa(Rsa::generate(2048)?)?;
    let mut name = X509NameBuilder::new()?;
    name.append_entry_by_text("CN", host)?;
    let name = name.build();
    let mut serial = BigNum::new()?;
    serial.rand(127, MsbOption::MAYBE_ZERO, false)?;
    let serial = serial.to_asn1_integer()?;
    let (not_before, not_after) = (Asn1Time::days_from_now(0)?, Asn1Time::days_from_now(3650)?);
    let mut cert = X509::builder()?;
    cert.set_version(2)?;
    cert.set_serial_number(&serial)?;
    cert.set_subject_name(&name)?;
    cert.set_issuer_name(&name)?;
    cert.set_pubkey(&key)?;
    cert.set_not_before(&not_before)?;
    cert.set_not_after(&not_after)?;
    cert.sign(&key, MessageDigest::sha256())?;
    Ok(Identity { certificate: cert.build(), key })
}

/// The acceptor for STARTTLS: TLS 1.0 and up, `ciphers` at security level 0,
/// no empty fragments before CBC records, no compression.
pub fn acceptor(identity: &Identity, ciphers: &str) -> Result<SslAcceptor, ErrorStack> {
    let mut builder = SslAcceptor::mozilla_intermediate_v5(SslMethod::tls_server())?;
    builder.clear_options(SslOptions::NO_TLSV1 | SslOptions::NO_TLSV1_1);
    builder.set_options(SslOptions::ALL | SslOptions::DONT_INSERT_EMPTY_FRAGMENTS | SslOptions::NO_COMPRESSION);
    builder.set_security_level(0);
    builder.set_min_proto_version(Some(SslVersion::TLS1))?;
    builder.set_cipher_list(ciphers)?;
    builder.set_private_key(&identity.key)?;
    builder.set_certificate(&identity.certificate)?;
    builder.check_private_key()?;
    Ok(builder.build())
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::pin::Pin;

    use openssl::ssl::{SslConnector, SslVerifyMode};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    use super::*;
    use crate::framer::Framer;
    use crate::stream::{Accounts, Out, Stream};

    struct Nobody;

    impl Accounts for Nobody {
        fn password(&self, _: &str) -> Option<String> {
            None
        }
    }

    /// Read until `end` appears, as a client waiting for a reply would.
    fn read_until(r: &mut impl Read, end: &str) -> String {
        let mut got = Vec::new();
        let mut buf = [0; 4096];
        while !String::from_utf8_lossy(&got).contains(end) {
            let n = r.read(&mut buf).unwrap();
            assert!(n > 0, "closed before {end}: {}", String::from_utf8_lossy(&got));
            got.extend_from_slice(&buf[..n]);
        }
        String::from_utf8(got).unwrap()
    }

    #[tokio::test]
    async fn a_tls_1_0_client_completes_starttls() {
        let acceptor = acceptor(&self_signed("127.0.0.1").unwrap(), DEFAULT_CIPHERS).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let server = tokio::spawn(async move {
            let (mut tcp, _) = listener.accept().await.unwrap();
            let mut stream = Stream::new("127.0.0.1", "stream1");
            let mut framer = Framer::default();
            let mut buf = [0; 4096];
            // Plaintext until the client asks for TLS.
            'plain: loop {
                let n = tcp.read(&mut buf).await.unwrap();
                framer.push(&buf[..n]);
                while let Some(frame) = framer.next_frame().unwrap() {
                    for out in stream.handle(frame, &Nobody).unwrap() {
                        match out {
                            Out::Send(s) => tcp.write_all(s.as_bytes()).await.unwrap(),
                            Out::StartTls => break 'plain,
                            Out::Available => {}
                            Out::Close => return,
                        }
                    }
                }
            }
            stream.tls_started();
            let ssl = openssl::ssl::Ssl::new(acceptor.context()).unwrap();
            let mut tls = tokio_openssl::SslStream::new(ssl, tcp).unwrap();
            Pin::new(&mut tls).accept().await.unwrap();
            let mut framer = Framer::default();
            let n = tls.read(&mut buf).await.unwrap();
            framer.push(&buf[..n]);
            let frame = framer.next_frame().unwrap().unwrap();
            for out in stream.handle(frame, &Nobody).unwrap() {
                if let Out::Send(s) = out {
                    tls.write_all(s.as_bytes()).await.unwrap();
                }
            }
            tls.shutdown().await.ok();
        });

        let client = tokio::task::spawn_blocking(move || {
            let open = "<?xml version='1.0' ?><stream:stream to='127.0.0.1' xmlns='jabber:client' xmlns:stream='http://etherx.jabber.org/streams' version='1.0'>";
            let mut tcp = std::net::TcpStream::connect(addr).unwrap();
            tcp.write_all(open.as_bytes()).unwrap();
            let plain = read_until(&mut tcp, "</stream:features>");
            tcp.write_all(b"<starttls xmlns='urn:ietf:params:xml:ns:xmpp-tls'/>").unwrap();
            read_until(&mut tcp, "<proceed xmlns=\"urn:ietf:params:xml:ns:xmpp-tls\"/>");

            // As transcript.py's client: TLS 1.0 only, old ciphers allowed.
            let mut connector = SslConnector::builder(SslMethod::tls_client()).unwrap();
            connector.set_verify(SslVerifyMode::NONE);
            connector.set_security_level(0);
            connector.set_min_proto_version(Some(SslVersion::TLS1)).unwrap();
            connector.set_max_proto_version(Some(SslVersion::TLS1)).unwrap();
            connector.set_cipher_list(DEFAULT_CIPHERS).unwrap();
            let mut tls =
                connector.build().configure().unwrap().verify_hostname(false).connect("127.0.0.1", tcp).unwrap();
            let version = tls.ssl().version_str().to_owned();
            let cipher = tls.ssl().current_cipher().unwrap().name().to_owned();
            tls.write_all(open.as_bytes()).unwrap();
            let secure = read_until(&mut tls, "</stream:features>");
            (plain, version, cipher, secure)
        });

        let (plain, version, cipher, secure) = client.await.unwrap();
        server.await.unwrap();
        assert!(plain.contains("<starttls "));
        assert_eq!(version, "TLSv1");
        // What the reference container negotiated with the same client settings.
        assert_eq!(cipher, "ECDHE-RSA-AES256-SHA");
        assert!(!secure.contains("starttls"), "features after TLS: {secure}");
        assert!(secure.contains(r#"<auth xmlns="http://jabber.org/features/iq-auth"/>"#));
    }
}
