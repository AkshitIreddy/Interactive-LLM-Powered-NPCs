const MAX_SSE_BUFFER_BYTES: usize = 2 * 1_048_576;
const MAX_SSE_EVENT_BYTES: usize = 1_048_576;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SseFrame {
    pub event: Option<String>,
    pub data: String,
}

#[derive(Default)]
pub(crate) struct SseDecoder {
    buffer: Vec<u8>,
    event_name: Option<String>,
    data_lines: Vec<String>,
    event_bytes: usize,
}

impl SseDecoder {
    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<SseFrame>, ()> {
        if self.buffer.len().saturating_add(bytes.len()) > MAX_SSE_BUFFER_BYTES {
            return Err(());
        }
        self.buffer.extend_from_slice(bytes);
        self.drain_lines(false)
    }

    pub fn finish(&mut self) -> Result<Vec<SseFrame>, ()> {
        let mut frames = self.drain_lines(true)?;
        if self.event_name.is_some() || !self.data_lines.is_empty() {
            if let Some(frame) = self.dispatch() {
                frames.push(frame);
            }
        }
        Ok(frames)
    }

    fn drain_lines(&mut self, at_eof: bool) -> Result<Vec<SseFrame>, ()> {
        let mut frames = Vec::new();
        while let Some(position) = self.buffer.iter().position(|byte| *byte == b'\n') {
            let mut line = self.buffer.drain(..=position).collect::<Vec<_>>();
            line.pop();
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            self.handle_line(&line, &mut frames)?;
        }
        if at_eof && !self.buffer.is_empty() {
            let line = std::mem::take(&mut self.buffer);
            self.handle_line(line.strip_suffix(b"\r").unwrap_or(&line), &mut frames)?;
        }
        Ok(frames)
    }

    fn handle_line(&mut self, line: &[u8], frames: &mut Vec<SseFrame>) -> Result<(), ()> {
        self.event_bytes = self.event_bytes.saturating_add(line.len());
        if self.event_bytes > MAX_SSE_EVENT_BYTES {
            return Err(());
        }
        if line.is_empty() {
            if let Some(frame) = self.dispatch() {
                frames.push(frame);
            }
            self.event_bytes = 0;
            return Ok(());
        }
        if line.starts_with(b":") {
            return Ok(());
        }
        let line = std::str::from_utf8(line).map_err(|_| ())?;
        let (field, mut value) = line.split_once(':').unwrap_or((line, ""));
        if let Some(stripped) = value.strip_prefix(' ') {
            value = stripped;
        }
        match field {
            "event" => self.event_name = Some(value.to_owned()),
            "data" => self.data_lines.push(value.to_owned()),
            _ => {}
        }
        Ok(())
    }

    fn dispatch(&mut self) -> Option<SseFrame> {
        let event = self.event_name.take();
        let data = std::mem::take(&mut self.data_lines).join("\n");
        if event.is_none() && data.is_empty() {
            None
        } else {
            Some(SseFrame { event, data })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_every_byte_boundary_and_crlf() {
        let fixture = b"event: delta\r\ndata: {\"text\":\"hello\"}\r\n\r\ndata: [DONE]\n\n";
        for split in 1..fixture.len() {
            let mut decoder = SseDecoder::default();
            let mut frames = decoder.push(&fixture[..split]).expect("first chunk");
            frames.extend(decoder.push(&fixture[split..]).expect("second chunk"));
            frames.extend(decoder.finish().expect("finish"));
            assert_eq!(frames.len(), 2, "split at byte {split}");
            assert_eq!(frames[0].event.as_deref(), Some("delta"));
            assert_eq!(frames[0].data, "{\"text\":\"hello\"}");
            assert_eq!(frames[1].data, "[DONE]");
        }
    }

    #[test]
    fn joins_multiline_data_and_ignores_comments() {
        let mut decoder = SseDecoder::default();
        let frames = decoder
            .push(b": keepalive\ndata: one\ndata: two\n\n")
            .expect("valid SSE");
        assert_eq!(frames[0].data, "one\ntwo");
    }
}
