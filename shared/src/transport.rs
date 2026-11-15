//! Datagram fragmentation so big packets (full soft-body snapshots or custom maps up to 512 KB) survive the internet.
//! Datagrams stay below ~1 KB, so no IP fragmentation / MTU problems on real networks.

use crate::protocol::Packet;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

/// payload bytes carried by one fragment
pub const CHUNK: usize = 1000;
/// packets up to this size are sent as-is
pub const MAX_SINGLE: usize = 1150;

static NEXT_ID: AtomicU32 = AtomicU32::new(1);

impl Packet {
    /// Encode into one or more datagrams, each safely below the internet MTU.
    pub fn encode_datagrams(&self) -> Result<Vec<Vec<u8>>, bincode::Error> {
        let bytes = self.encode()?;
        if bytes.len() <= MAX_SINGLE {
            return Ok(vec![bytes]);
        }
        let count = (bytes.len() + CHUNK - 1) / CHUNK;
        if count > 1024 {
            return Err(Box::new(bincode::ErrorKind::Custom("packet too large".into())));
        }
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let mut out = Vec::with_capacity(count);
        for (i, c) in bytes.chunks(CHUNK).enumerate() {
            out.push(Packet::Fragment { id, idx: i as u16, count: count as u16, data: c.to_vec() }.encode()?);
        }
        Ok(out)
    }
}

struct Partial {
    chunks: Vec<Option<Vec<u8>>>,
    got: usize,
    t: Instant,
}

/// Re-assembles fragments per sender. Feed every received datagram through `feed`.
#[derive(Default)]
pub struct Reassembler {
    parts: HashMap<(SocketAddr, u32), Partial>,
}

impl Reassembler {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns a complete packet when one is available (a plain packet, or the last missing fragment).
    pub fn feed(&mut self, from: SocketAddr, data: &[u8]) -> Option<Packet> {
        let p = Packet::decode(data).ok()?;
        let (id, idx, count, data) = match p {
            Packet::Fragment { id, idx, count, data } => (id, idx, count, data),
            other => return Some(other),
        };
        if count == 0 || idx >= count || count > 1024 {
            return None;
        }
        if self.parts.len() > 256 {
            self.parts.retain(|_, v| v.t.elapsed() < Duration::from_secs(5));
            if self.parts.len() > 256 {
                self.parts.clear();
            }
        }
        let e = self.parts.entry((from, id)).or_insert_with(|| Partial { chunks: vec![None; count as usize], got: 0, t: Instant::now() });
        if e.chunks.len() != count as usize {
            return None;
        }
        if e.chunks[idx as usize].is_none() {
            e.chunks[idx as usize] = Some(data);
            e.got += 1;
        }
        if e.got < e.chunks.len() {
            return None;
        }
        let done = self.parts.remove(&(from, id))?;
        let mut all = Vec::new();
        for c in done.chunks {
            all.extend(c?);
        }
        match Packet::decode(&all).ok()? {
            Packet::Fragment { .. } => None,
            inner => Some(inner),
        }
    }
}
