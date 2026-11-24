//! The engine of the web page: the JSON API of `pantheon::api` behind three raw exports.
//!
//! The page writes a UTF-8 JSON request into memory from `pb_alloc`, calls `pb_call` and reads
//! back `[u32 little-endian length][UTF-8 JSON answer]`, then frees both buffers with `pb_free`.
//! The game data is compiled in, so the page and `pantheon-battler fight` agree as long as they
//! show the same data hash.

use pantheon::api;
use pantheon::data::Data;
use std::sync::{Once, OnceLock};

static DATA: OnceLock<Data> = OnceLock::new();
static HOOK: Once = Once::new();

#[link(wasm_import_module = "env")]
unsafe extern "C" {
    /// Receives the panic message (UTF-8) before the module aborts.
    fn host_error(ptr: *const u8, len: usize);
}

fn data() -> &'static Data {
    DATA.get_or_init(|| {
        Data::from_strs(include_str!("../../data/units.toml"), include_str!("../../data/rules.toml"))
            .expect("données du jeu illisibles")
    })
}

/// A buffer of `len` bytes for the page to write into.
#[unsafe(no_mangle)]
pub extern "C" fn pb_alloc(len: usize) -> *mut u8 {
    Box::into_raw(vec![0u8; len].into_boxed_slice()) as *mut u8
}

/// Frees a buffer from `pb_alloc` or `pb_call` (`len` is its full size).
///
/// # Safety
/// `ptr` and `len` must come from `pb_alloc`, or from `pb_call` with the 4-byte header counted.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pb_free(ptr: *mut u8, len: usize) {
    if !ptr.is_null() {
        drop(unsafe { Box::from_raw(std::ptr::slice_from_raw_parts_mut(ptr, len)) });
    }
}

/// Answers the JSON request at `ptr`: returns `[u32 LE length][JSON]`.
///
/// # Safety
/// `ptr` and `len` must describe a buffer from `pb_alloc`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pb_call(ptr: *const u8, len: usize) -> *mut u8 {
    HOOK.call_once(|| {
        std::panic::set_hook(Box::new(|info| {
            let msg = info.to_string();
            unsafe { host_error(msg.as_ptr(), msg.len()) };
        }))
    });
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
    let out = match std::str::from_utf8(bytes) {
        Ok(request) => api::handle(data(), request),
        Err(_) => r#"{"ok":false,"errors":["Requête illisible : UTF-8 invalide"]}"#.to_string(),
    };
    let mut buf = Vec::with_capacity(4 + out.len());
    buf.extend_from_slice(&(out.len() as u32).to_le_bytes());
    buf.extend_from_slice(out.as_bytes());
    Box::into_raw(buf.into_boxed_slice()) as *mut u8
}
