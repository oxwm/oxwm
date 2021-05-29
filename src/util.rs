use std::convert::TryInto;

use x11rb::protocol::xproto;

/// Convert an `EventMask` to a `u16`. Note that not every event mask is
/// convertible
pub fn event_mask_to_u16(mask: xproto::EventMask) -> u16 {
    let mask = u32::from(mask);
    let mask: u16 = mask.try_into().unwrap();
    mask
}
