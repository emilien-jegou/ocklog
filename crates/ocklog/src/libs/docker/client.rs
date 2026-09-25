use std::path::Path;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

pub const DEFAULT_SOCKET_PATH: &str = "/var/run/docker.sock";

#[derive(Debug, Clone)]
pub struct DockerTransport {
    socket_path: String,
}

impl DockerTransport {
    pub fn new(socket_path: impl Into<String>) -> Self {
        Self { socket_path: socket_path.into() }
    }

    pub fn default_socket() -> Self {
        Self::new(DEFAULT_SOCKET_PATH)
    }

    pub fn is_available(&self) -> bool {
        Path::new(&self.socket_path).exists()
    }

    pub async fn connect(&self) -> eyre::Result<UnixStream> {
        Ok(UnixStream::connect(&self.socket_path).await?)
    }

    pub async fn send_get_request(&self, endpoint: &str, upgrade: bool) -> eyre::Result<BufReader<UnixStream>> {
        let mut stream = self.connect().await?;
        let connection_header = if upgrade {
            "Upgrade: tcp\r\nConnection: Upgrade\r\n"
        } else {
            "Connection: close\r\n"
        };
        let request = format!(
            "GET {} HTTP/1.1\r\nHost: localhost\r\n{}\r\n",
            endpoint, connection_header
        );
        stream.write_all(request.as_bytes()).await?;
        Ok(BufReader::new(stream))
    }

    pub async fn send_post_request(&self, endpoint: &str) -> eyre::Result<()> {
        let mut stream = self.connect().await?;
        let request = format!(
            "POST {} HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            endpoint
        );
        stream.write_all(request.as_bytes()).await?;
        let mut buf = [0u8; 128];
        let _ = stream.read(&mut buf).await;
        Ok(())
    }
}

pub struct StreamFlags {
    pub is_chunked: bool,
    pub is_multiplexed: bool,
}

impl StreamFlags {
    pub async fn detect<R: AsyncBufReadExt + Unpin>(reader: &mut R) -> Self {
        let mut header_buf = String::new();
        let mut is_chunked = false;
        let mut is_multiplexed = true;

        while let Ok(bytes) = reader.read_line(&mut header_buf).await {
            if bytes == 0 || header_buf == "\r\n" || header_buf == "\n" {
                break;
            }
            let lower = header_buf.to_lowercase();
            if lower.contains("transfer-encoding") && lower.contains("chunked") {
                is_chunked = true;
            }
            if lower.contains("application/vnd.docker.raw-stream") {
                is_multiplexed = false;
            }
            header_buf.clear();
        }

        Self { is_chunked, is_multiplexed }
    }
}

pub async fn read_chunked_exact<R: AsyncBufReadExt + Unpin>(
    reader: &mut R,
    buf: &mut [u8],
    chunk_rem: &mut usize,
) -> std::io::Result<()> {
    let mut filled = 0;
    while filled < buf.len() {
        if *chunk_rem == 0 {
            *chunk_rem = read_chunk_size(reader).await?;
        }
        let to_read = (buf.len() - filled).min(*chunk_rem);
        reader.read_exact(&mut buf[filled..filled + to_read]).await?;
        filled += to_read;
        *chunk_rem -= to_read;

        if *chunk_rem == 0 {
            let mut crlf = [0u8; 2];
            let _ = reader.read_exact(&mut crlf).await;
        }
    }
    Ok(())
}

async fn read_chunk_size<R: AsyncBufReadExt + Unpin>(reader: &mut R) -> std::io::Result<usize> {
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line).await? == 0 {
            return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "EOF in chunk header"));
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let hex = trimmed.split(';').next().unwrap_or("0");
        let size = usize::from_str_radix(hex, 16)
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid hex chunk"))?;
        if size == 0 {
            return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "Zero-size chunk"));
        }
        return Ok(size);
    }
}
