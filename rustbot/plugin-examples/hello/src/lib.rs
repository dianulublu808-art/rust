use serde::{Deserialize, Serialize};
use std::slice;

#[derive(Deserialize)]
struct Input {
    command: String,
    args: Vec<String>,
    args_raw: String,
    message_text: String,
    message_id: i32,
}

#[derive(Serialize)]
struct Output {
    text: String,
}

/// Host allocates this many bytes in the module's exported linear memory.
#[no_mangle]
pub extern "C" fn alloc(len: usize) -> *mut u8 {
    let buffer = Vec::<u8>::with_capacity(len);
    let ptr = buffer.as_ptr() as *mut u8;
    std::mem::forget(buffer);
    ptr
}

/// ABI: `handle(input_ptr, input_len)` returns `(output_ptr << 32) | output_len`.
///
/// The host only exposes JSON input and accepts JSON text output. The module has
/// no WASI imports or access to Telegram/network/filesystem APIs.
#[no_mangle]
pub unsafe extern "C" fn handle(input_ptr: i32, input_len: i32) -> i64 {
    let input = if input_ptr < 0 || input_len < 0 {
        None
    } else {
        // The host bounds-checks pointers and lengths against exported memory.
        Some(slice::from_raw_parts(
            input_ptr as usize as *const u8,
            input_len as usize,
        ))
    };

    let text = match input.and_then(|bytes| serde_json::from_slice::<Input>(bytes).ok()) {
        Some(input) => {
            let name = input
                .args
                .first()
                .cloned()
                .filter(|name| !name.trim().is_empty())
                .unwrap_or_else(|| "друг".to_owned());
            format!(
                "Привет, {name}! Команда: {}. Сообщение #{}: {}",
                input.command,
                input.message_id,
                if input.args_raw.is_empty() {
                    input.message_text
                } else {
                    input.args_raw
                }
            )
        }
        None => "Не удалось разобрать входные данные плагина".to_owned(),
    };

    let output = match serde_json::to_vec(&Output { text }) {
        Ok(output) => output,
        Err(_) => return 0,
    };
    let output_ptr = output.as_ptr() as u32;
    let output_len = output.len() as u32;
    std::mem::forget(output);

    (((output_ptr as u64) << 32) | output_len as u64) as i64
}
