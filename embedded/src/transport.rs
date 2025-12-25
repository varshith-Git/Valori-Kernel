// Packet Headers
const SYNC_WORD: [u8; 4] = [0x55, 0xAA, 0x55, 0xAA];
const TYPE_PROOF: u8 = 0x01;
const TYPE_SNAPSHOT: u8 = 0x02;
pub const TYPE_WAL: u8 = 0x03;
pub const TYPE_ERR: u8 = 0xEE;

/// Simulated UART write
/// In production, this writes to TX register.
fn uart_write(byte: u8) {
    // Hardware specific implementation.
    // For now, no-op or ITM/Semihosting hook.
    // core::hint::black_box(byte);
    unsafe { 
        // Cast integer address to pointer
        let tx_reg = 0x4000_0000 as *mut u32; 
        core::ptr::write_volatile(tx_reg, byte as u32);    
    } 
}

fn send_chunk(type_id: u8, data: &[u8]) {
    // 1. SYNC
    for b in SYNC_WORD.iter() { uart_write(*b); }
    
    // 2. TYPE
    uart_write(type_id);

    // 3. LEN (u32 LE)
    let len = data.len() as u32;
    for b in len.to_le_bytes().iter() { uart_write(*b); }

    // 4. PAYLOAD
    for b in data.iter() { uart_write(*b); }
}

pub fn export_proof(proof_json: &[u8]) {
    send_chunk(TYPE_PROOF, proof_json);
}

pub fn export_snapshot(data: &[u8]) {
    const CHUNK_SIZE: usize = 256;
    for chunk in data.chunks(CHUNK_SIZE) {
        send_chunk(TYPE_SNAPSHOT, chunk);
    }
}

pub fn export_error(err_code: &[u8]) {
    send_chunk(TYPE_ERR, err_code);
}
