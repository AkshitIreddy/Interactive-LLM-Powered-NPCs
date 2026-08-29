use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

#[derive(Debug, thiserror::Error)]
pub enum FrameError {
    #[error("control stream ended")]
    EndOfStream,
    #[error("control frame length is invalid")]
    InvalidLength,
    #[error("control stream I/O failed")]
    Io,
}

pub async fn read_frame<R>(reader: &mut R, maximum: usize) -> Result<Vec<u8>, FrameError>
where
    R: AsyncRead + Unpin,
{
    let mut length = [0_u8; 4];
    match reader.read_exact(&mut length).await {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => {
            return Err(FrameError::EndOfStream)
        }
        Err(_) => return Err(FrameError::Io),
    }
    let body_len = u32::from_le_bytes(length) as usize;
    if body_len == 0 || body_len.saturating_add(4) > maximum {
        return Err(FrameError::InvalidLength);
    }
    let mut frame = Vec::with_capacity(body_len + 4);
    frame.extend_from_slice(&length);
    frame.resize(body_len + 4, 0);
    reader
        .read_exact(&mut frame[4..])
        .await
        .map_err(|_| FrameError::Io)?;
    Ok(frame)
}

pub async fn write_frame<W>(writer: &mut W, body: &[u8], maximum: usize) -> Result<(), FrameError>
where
    W: AsyncWrite + Unpin,
{
    if body.is_empty() || body.len().saturating_add(4) > maximum {
        return Err(FrameError::InvalidLength);
    }
    let length = u32::try_from(body.len()).map_err(|_| FrameError::InvalidLength)?;
    writer
        .write_all(&length.to_le_bytes())
        .await
        .map_err(|_| FrameError::Io)?;
    writer.write_all(body).await.map_err(|_| FrameError::Io)?;
    writer.flush().await.map_err(|_| FrameError::Io)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn bounded_frame_round_trip() {
        let (mut client, mut server) = tokio::io::duplex(128);
        let task = tokio::spawn(async move { write_frame(&mut client, b"fixture", 64).await });
        let frame = read_frame(&mut server, 64).await.unwrap();
        assert_eq!(&frame[4..], b"fixture");
        task.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn oversized_prefix_is_rejected_before_allocation() {
        let (mut client, mut server) = tokio::io::duplex(16);
        tokio::spawn(async move {
            client.write_all(&u32::MAX.to_le_bytes()).await.unwrap();
        });
        assert!(matches!(
            read_frame(&mut server, 1024).await,
            Err(FrameError::InvalidLength)
        ));
    }
}
