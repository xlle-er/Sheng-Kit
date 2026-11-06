//! Cuts the PCM16 stream into frames of exactly 80 ms.

/// Bytes in 80 ms of 16 kHz mono PCM16.
pub const FRAME_BYTES: usize = 16_000 * 2 * 80 / 1000;

/// Holds the incomplete tail between pushes. The buffer never exceeds one frame.
#[derive(Debug, Default)]
pub struct Framer {
    tail: Vec<u8>,
}

impl Framer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends `pcm` and returns every complete frame, in order.
    pub fn push(&mut self, pcm: &[u8]) -> Vec<Vec<u8>> {
        let mut frames = Vec::new();
        let mut rest = pcm;
        if !self.tail.is_empty() {
            let needed = FRAME_BYTES - self.tail.len();
            let take = needed.min(rest.len());
            self.tail.extend_from_slice(&rest[..take]);
            rest = &rest[take..];
            if self.tail.len() < FRAME_BYTES {
                return frames;
            }
            frames.push(std::mem::take(&mut self.tail));
        }
        let (chunks, remainder) = rest.as_chunks::<FRAME_BYTES>();
        frames.extend(chunks.iter().map(|frame| frame.to_vec()));
        self.tail = remainder.to_vec();
        frames
    }

    /// At the end of input: the incomplete tail padded with silence to a full frame, if any.
    pub fn flush(&mut self) -> Option<Vec<u8>> {
        if self.tail.is_empty() {
            return None;
        }
        let mut frame = std::mem::take(&mut self.tail);
        frame.resize(FRAME_BYTES, 0);
        Some(frame)
    }

    pub fn pending(&self) -> usize {
        self.tail.len()
    }
}
