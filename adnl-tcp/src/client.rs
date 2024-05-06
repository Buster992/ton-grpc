use std::time::Duration;
use anyhow::{anyhow, bail};
use futures::StreamExt;
use rand::Rng;
use sha2::{Digest, Sha256};
use aes::cipher::StreamCipher;
use tokio::net::{TcpStream, ToSocketAddrs};
use tokio_util::codec::Framed;
use tokio::io::AsyncWriteExt;
use crate::codec::PacketCodec;
use crate::key::Ed25519Key;
use crate::connection::Connection;

pub type ServerKey = [u8; 32];

pub struct Client;

impl Client {
    pub async fn connect<A: ToSocketAddrs>(addr: A, server_key: &ServerKey) -> anyhow::Result<Connection> {
        let mut stream = TcpStream::connect(addr).await?;

        let (aes_basis, aes_bases_checksum) = Self::generate_aes_basis();

        let server_key = Ed25519Key::from_public_key_bytes(server_key)?;
        let client_key = Ed25519Key::generate();
        let shared_key = client_key.shared_key(server_key.public_key())?;

        tracing::debug!(server_key_id = ?server_key.id());
        tracing::debug!(shared_key = ?shared_key);
        tracing::debug!(aes_basis = ?aes_basis);

        let mut aes_basis_encrypted = [0u8; 160];
        crate::codec::build_cipher(&shared_key, &aes_bases_checksum)
            .apply_keystream_b2b(&aes_basis, &mut aes_basis_encrypted)
            .map_err(|e| anyhow!(e))?;

        tracing::debug!(aes_basis = ?aes_basis);

        let handshake_packet = [
            server_key.id().as_slice(),
            client_key.public_key().as_bytes(),
            aes_bases_checksum.as_slice(),
            aes_basis_encrypted.as_slice()
        ].concat();

        tracing::debug!(handshake_packet = ?handshake_packet);

        stream.write_all(handshake_packet.as_slice()).await?;
        stream.flush().await?;

        let codec = PacketCodec::from_bytes_as_client(&aes_basis);
        let mut framed = Framed::new(stream, codec);

        let packet = tokio::time::timeout(
            Duration::from_secs(5),
            framed.next()
        ).await?.ok_or(anyhow!("missed empty packet"))??;

        tracing::info!(packet = ?packet, "received packet");
        if packet.is_empty() {
            tracing::info!("handshake ok");
        } else {
            bail!("empty packet expected")
        }

        Ok(Connection::new(framed))
    }

    fn generate_aes_basis() -> ([u8; 160], [u8; 32]) {
        let mut aes_basis = [0u8; 160];
        rand::thread_rng().fill(aes_basis.as_mut_slice());

        let checksum = Sha256::digest(aes_basis).into();

        (aes_basis, checksum)
    }
}

#[cfg(test)]
mod tests {
    use std::net::{Ipv4Addr, SocketAddrV4};
    use base64::Engine;
    use futures::SinkExt;
    use tracing_test::traced_test;
    use crate::ping::{is_pong_packet, ping_packet};
    use super::*;

    #[traced_test]
    #[tokio::test]
    #[ignore]
    async fn client_connect() -> anyhow::Result<()> {
        let _ = provided_client().await?;

        Ok(())
    }

    #[traced_test]
    #[tokio::test]
    #[ignore]
    async fn client_connect_wrong_key() -> anyhow::Result<()> {
        let ip: i32 = -2018147075;
        let ip = Ipv4Addr::from(ip as u32);
        let port = 46529;
        let key: ServerKey = (0..32).collect::<Vec<_>>().try_into().unwrap();

        tracing::info!("Connecting to {}:{} with key {:?}", ip, port, key);

        let client = Client::connect(SocketAddrV4::new(ip, port), &key).await;

        assert!(client.is_err());
        assert_eq!(client.err().unwrap().to_string(), "missed empty packet".to_string());

        Ok(())
    }

    #[traced_test]
    #[tokio::test]
    #[ignore]
    async fn client_ping() -> anyhow::Result<()> {
        let mut client = provided_client().await?;

        let sent = client.send(ping_packet()).await;
        let received = client.next().await.unwrap()?;

        assert!(sent.is_ok());
        assert!(is_pong_packet(&received));

        Ok(())
    }

    async fn provided_client() -> anyhow::Result<Connection> {
        let ip: i32 = -2018147075;
        let ip = Ipv4Addr::from(ip as u32);
        let port = 46529;
        let key: ServerKey = base64::engine::general_purpose::STANDARD.decode("jLO6yoooqUQqg4/1QXflpv2qGCoXmzZCR+bOsYJ2hxw=")?.as_slice().try_into()?;

        tracing::info!("Connecting to {}:{} with key {:?}", ip, port, key);

        let connection = Client::connect(SocketAddrV4::new(ip, port), &key).await?;

        Ok(connection)
    }
}
