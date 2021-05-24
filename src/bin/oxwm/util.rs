//! Miscellaneous utilities for interfacing with X.

use std::convert::TryInto;

use x11rb::connection::Connection;
use x11rb::protocol::xproto;
use x11rb::protocol::xproto::ConnectionExt;

use crate::Result;

pub fn event_mask_to_u16(mask: xproto::EventMask) -> u16 {
    // HACK There seems (?) to be no canonical way to convert an EventMask to a
    // u16. So, instead, we do this:
    let mask = u32::from(mask);
    let mask: u16 = mask.try_into().unwrap();
    mask
}

pub fn with_grabbed_server<Conn, A, B, F>(conn: &Conn, f: F) -> Result<B>
where
    Conn: Connection,
    A: Into<Result<B>>,
    F: FnOnce() -> A,
{
    conn.grab_server()?.check()?;
    let x = f().into();
    conn.ungrab_server()?.check()?;
    x
}
