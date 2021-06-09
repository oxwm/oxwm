//! Various assorted utility functions.

use std::convert::TryFrom;
use std::convert::TryInto;

use x11rb::protocol::xproto;

use libc::{c_char, c_ulong};
use std::ffi::CString;

/// Convert an `EventMask` to a `u16`. Note that not every event mask is
/// convertible
pub fn event_mask_to_u16(mask: xproto::EventMask) -> u16 {
    let mask = u32::from(mask);
    let mask: u16 = mask.try_into().unwrap();
    mask
}

/// Lookup the numeric value for a given `Keysym`'s text name, e.g. "Shift_L" -> 50
/// Returns `None` if the given `key_name` is not the name of a valid Keysym or
/// contains `null` values.
pub fn keysym_from_name(key_name: &str) -> Option<xproto::Keysym> {
    let sym64: u64;

    // Need: The X11 library is written in C, at this time we have been
    //       unable to find a working rust crate that offers equivalent
    //       functionality or a binding to the `XStringToKeysym` function.
    //       Rather than reproduce this function in rust, we choose to call
    //       the X11 C library directly to perform the name to value lookup.
    //
    // Safety: This block will create a new C style null-terminated string
    //         on the heap and pass a pointer to that string to the X11 C
    //         library function. The string behind this pointer is considered
    //         read-only, and undefined behavior may result if the C function
    //         attempts to modify the strings contents.
    //
    //         The assumption is made that XStringToKeysym in the X11 library
    //         will not attempt to modify the memory we pass to it.
    //
    //         The C string is not reused after it has been passed to
    //         XStringToKeysym.
    unsafe {
        let null_terminated_result = CString::new(key_name);

        if let Ok(null_terminated) = null_terminated_result {
            sym64 = XStringToKeysym(null_terminated.as_ptr());
        } else {
            return None;
        }
    }

    //While the X11 library call returns a u64, xproto::Keysym is a u32.
    //Convert to u32 or return None if the keysym value returned by the
    //C library is too large.
    //Return None if the library call returned 0 aka `NoSymbol`.
    match sym64 {
        0 => None,
        sym64 => {
            if let Ok(ret_symbol) = u32::try_from(sym64) {
                Some(ret_symbol)
            } else {
                None
            }
        }
    }
}

/// An FFI call to the X11 C library function for converting from Keysym names
/// to Keysym values. This is unsafe code. 'symbol' _must_ be a pointer to a
/// null terminated C style string such as is produced by std::ffi::Cstring.
#[link(name = "X11")]
extern "C" {
    fn XStringToKeysym(symbol_name: *const c_char) -> c_ulong;
}

/// Query the running X11 server for the Keycode currently mapped, if any, to a Keysym.
/// Unlike the majority of code in oxwm, this function uses the `xcb` and `xcb_util`
/// crates instead of `x11rb` to interfacing with an X11 server.
pub fn keycode_from_keysym(keysym_value: xproto::Keysym) -> Option<xproto::Keycode> {
    if let Ok((xcb_conn, _screen)) = xcb::Connection::connect(None) {
        let converter = xcb_util::keysyms::KeySymbols::new(&xcb_conn);
        match converter.get_keycode(keysym_value).next() {
            None => None,
            Some(0) => None,
            Some(key_code) => Some(key_code),
        }
    } else {
        None
    }
}
