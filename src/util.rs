use std::convert::TryInto;
use x11rb::protocol::xproto;

pub fn event_mask_to_u16(mask: xproto::EventMask) -> u16 {
    // HACK There seems (?) to be no canonical way to convert an EventMask to a
    // u16. So, instead, we do this:
    let mask = u32::from(mask);
    let mask: u16 = mask.try_into().unwrap();
    mask
}
