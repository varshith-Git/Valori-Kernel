// Framing: [SYNC:4][TYPE:1][LEN:4 LE][PAYLOAD:LEN]
const SYNC_WORD: [u8; 4] = [0x55, 0xAA, 0x55, 0xAA];
pub const TYPE_WAL:           u8 = 0x03;
pub const TYPE_SEARCH:        u8 = 0x04;
pub const TYPE_PROOF:         u8 = 0x01;
pub const TYPE_SNAPSHOT:      u8 = 0x02;
pub const TYPE_SEARCH_RESULT: u8 = 0x05;
pub const TYPE_INFER:         u8 = 0x06; // prompt tokens → run INT inference
pub const TYPE_INFER_RESULT:  u8 = 0x07; // output tokens + BLAKE3 receipt + Valori proof
pub const TYPE_ERR:           u8 = 0xEE;

// ── TX register ──────────────────────────────────────────────────────────────
// STM32F4 USART2 TX = 0x4000_4400
// lm3s6965evb UART0 DR = 0x4000_C000  (override with --features qemu)
#[cfg(not(feature = "qemu"))]
const UART_TX: usize = 0x4000_4400;
#[cfg(feature = "qemu")]
const UART_TX: usize = 0x4000_C000;

// ── RX register ──────────────────────────────────────────────────────────────
#[cfg(not(feature = "qemu"))]
const UART_RX: usize = 0x4000_4404;
#[cfg(feature = "qemu")]
const UART_RX: usize = 0x4000_C000;

// ── TX ───────────────────────────────────────────────────────────────────────

#[inline(always)]
fn uart_write(byte: u8) {
    unsafe { core::ptr::write_volatile(UART_TX as *mut u32, byte as u32); }
}

fn send_framed(type_id: u8, data: &[u8]) {
    for b in SYNC_WORD.iter()                   { uart_write(*b); }
    uart_write(type_id);
    for b in (data.len() as u32).to_le_bytes()  { uart_write(b); }
    for b in data.iter()                         { uart_write(*b); }
}

pub fn export_proof(proof_json: &[u8])    { send_framed(TYPE_PROOF, proof_json); }
pub fn export_snapshot(data: &[u8]) {
    for chunk in data.chunks(256) { send_framed(TYPE_SNAPSHOT, chunk); }
}
pub fn export_error(code: &[u8])          { send_framed(TYPE_ERR, code); }
pub fn export_search_result(data: &[u8]) { send_framed(TYPE_SEARCH_RESULT, data); }
pub fn export_infer_result(data: &[u8])  { send_framed(TYPE_INFER_RESULT, data); }

// ── RX ring buffer ───────────────────────────────────────────────────────────

const RX_BUF_SIZE: usize = 512;

pub struct RxBuf {
    buf:  [u8; RX_BUF_SIZE],
    head: usize,
    tail: usize,
}

impl RxBuf {
    pub const fn new() -> Self {
        Self { buf: [0u8; RX_BUF_SIZE], head: 0, tail: 0 }
    }

    pub fn push(&mut self, byte: u8) {
        let next = (self.head + 1) % RX_BUF_SIZE;
        if next != self.tail {
            self.buf[self.head] = byte;
            self.head = next;
        }
        // Drop on overflow — host must respect flow control.
    }

    pub fn pop(&mut self) -> Option<u8> {
        if self.head == self.tail { return None; }
        let b = self.buf[self.tail];
        self.tail = (self.tail + 1) % RX_BUF_SIZE;
        Some(b)
    }

    pub fn len(&self) -> usize {
        (self.head + RX_BUF_SIZE - self.tail) % RX_BUF_SIZE
    }
}

// ── Low-level RX helpers ─────────────────────────────────────────────────────

#[inline(always)]
fn uart_read_byte() -> u8 {
    unsafe { core::ptr::read_volatile(UART_RX as *const u8) }
}

fn recv_into(rx: &mut RxBuf, n: usize) {
    while rx.len() < n { rx.push(uart_read_byte()); }
}

fn drain_into(rx: &mut RxBuf, dst: &mut [u8]) {
    for slot in dst.iter_mut() { *slot = rx.pop().unwrap_or(0); }
}

// ── Generic framed packet receiver ───────────────────────────────────────────

pub enum PacketKind {
    Wal,
    Search,
    Infer,   // TYPE_INFER: run INT inference + store receipt in Valori
    Unknown,
}

pub enum RecvError {
    BadSync,
    Overflow,
}

pub struct ReceivedPacket {
    pub kind: PacketKind,
    pub len:  usize,
}

/// Block until one complete framed packet arrives.
/// Payload is written into `out`; returns kind + byte count.
/// On `BadSync` the caller should discard one byte from the stream and retry.
pub fn recv_packet(rx: &mut RxBuf, out: &mut [u8]) -> Result<ReceivedPacket, RecvError> {
    // 1. Sync word
    recv_into(rx, 4);
    let mut sync = [0u8; 4];
    drain_into(rx, &mut sync);
    if sync != SYNC_WORD { return Err(RecvError::BadSync); }

    // 2. Type
    recv_into(rx, 1);
    let pkt_type = rx.pop().unwrap_or(0);

    // 3. Length
    recv_into(rx, 4);
    let mut lb = [0u8; 4];
    drain_into(rx, &mut lb);
    let len = u32::from_le_bytes(lb) as usize;

    if len > out.len() { return Err(RecvError::Overflow); }

    // 4. Payload
    recv_into(rx, len);
    drain_into(rx, &mut out[0..len]);

    let kind = match pkt_type {
        TYPE_WAL    => PacketKind::Wal,
        TYPE_SEARCH => PacketKind::Search,
        TYPE_INFER  => PacketKind::Infer,
        _           => PacketKind::Unknown,
    };

    Ok(ReceivedPacket { kind, len })
}
