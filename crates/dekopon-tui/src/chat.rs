//! Bounded JSON-line client of the owner-only development transport, not an agent loop.

use std::{
    io,
    os::unix::fs::{FileTypeExt as _, MetadataExt as _},
    path::Path,
    time::Duration,
};

use dekopon_core::ExternalSubject;
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt as _, AsyncReadExt as _, AsyncWriteExt as _, BufReader},
    net::UnixStream,
};

/// Request and response budget, including the terminating newline.
pub const MAX_LINE_BYTES: usize = 64 * 1024;
const TIMEOUT: Duration = Duration::from_secs(120);

/// One ordered conversation. Any failed exchange closes it: a late answer cannot become the
/// answer to the next request. No requests are retried automatically.
pub struct ChatClient {
    stream: Option<BufReader<UnixStream>>,
    subject: ExternalSubject,
    channel: String,
}

impl ChatClient {
    /// Connects only to a same-owner, non-symlink 0600 socket and verifies the actual peer UID.
    pub async fn connect(
        path: &Path,
        subject: ExternalSubject,
        channel: String,
    ) -> Result<Self, ChatError> {
        let metadata = std::fs::symlink_metadata(path).map_err(ChatError::Io)?;
        let uid = rustix::process::geteuid().as_raw();
        if !metadata.file_type().is_socket()
            || metadata.uid() != uid
            || metadata.mode() & 0o7777 != 0o600
        {
            return Err(ChatError::UnsafeSocket);
        }
        let parent_path = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let parent = std::fs::metadata(parent_path).map_err(ChatError::Io)?;
        if !parent.is_dir() || parent.uid() != uid || parent.mode() & 0o022 != 0 {
            return Err(ChatError::UnsafeSocket);
        }
        let stream = tokio::time::timeout(TIMEOUT, UnixStream::connect(path))
            .await
            .map_err(ChatError::Timeout)?
            .map_err(ChatError::Io)?;
        if stream.peer_cred().map_err(ChatError::Io)?.uid() != uid {
            return Err(ChatError::UnsafeSocket);
        }
        Ok(Self {
            stream: Some(BufReader::new(stream)),
            subject,
            channel,
        })
    }

    /// Sends one line and receives exactly one bounded newline-terminated reply.
    pub async fn send(&mut self, text: &str) -> Result<String, ChatError> {
        self.exchange(text, TIMEOUT).await
    }

    async fn exchange(&mut self, text: &str, timeout: Duration) -> Result<String, ChatError> {
        // Bound each input before JSON escaping allocates an envelope (escaping grows at most 6x).
        let subject = self.subject.to_string();
        if [text, &subject, &self.channel]
            .iter()
            .any(|part| part.len() >= MAX_LINE_BYTES)
        {
            return Err(ChatError::RequestTooLarge);
        }
        let mut request =
            json!({"subject": subject, "channel": self.channel, "text": text}).to_string();
        request.push('\n');
        if request.len() > MAX_LINE_BYTES {
            return Err(ChatError::RequestTooLarge);
        }
        let mut stream = self.stream.take().ok_or(ChatError::Closed)?;
        let reply = tokio::time::timeout(timeout, async {
            stream
                .get_mut()
                .write_all(request.as_bytes())
                .await
                .map_err(ChatError::Io)?;
            let mut line = Vec::new();
            (&mut stream)
                .take(MAX_LINE_BYTES as u64)
                .read_until(b'\n', &mut line)
                .await
                .map_err(ChatError::Io)?;
            if !line.ends_with(b"\n") {
                return Err(if line.len() == MAX_LINE_BYTES {
                    ChatError::ReplyTooLarge
                } else {
                    ChatError::Closed
                });
            }
            let value: Value = serde_json::from_slice(&line)
                .map_err(|_payload_error| ChatError::MalformedReply)?;
            // The gateway may also supply images. This text-only client deliberately does not
            // decode, save, or render those attachments.
            value
                .get("reply")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or(ChatError::MalformedReply)
        })
        .await
        .map_err(ChatError::Timeout)??;
        self.stream = Some(stream);
        Ok(reply)
    }
}

/// Payload-free diagnostics: malformed server data never becomes an error message.
#[derive(Debug, thiserror::Error)]
pub enum ChatError {
    /// Unsafe socket metadata or peer.
    #[error("chat requires an owner-only 0600 socket and same-UID gateway")]
    UnsafeSocket,
    /// Local operating-system failure.
    #[error("chat socket I/O failed (kind: {:?}, errno: {:?})", .0.kind(), .0.raw_os_error())]
    Io(#[source] io::Error),
    /// JSON envelope exceeds the gateway limit.
    #[error("chat request exceeds the 65536-byte JSON-line limit")]
    RequestTooLarge,
    /// Reply exceeds the client limit.
    #[error("chat reply exceeds the 65536-byte JSON-line limit; connection closed")]
    ReplyTooLarge,
    /// Peer hung up or a prior exchange failed.
    #[error("chat connection closed; restart chat to reconnect")]
    Closed,
    /// Response was not a JSON object with a textual reply.
    #[error("malformed chat reply; connection closed")]
    MalformedReply,
    /// Bounded wait elapsed.
    #[error("chat timed out; connection closed (the gateway may still finish the turn)")]
    Timeout(#[source] tokio::time::error::Elapsed),
}

#[cfg(test)]
mod tests;
