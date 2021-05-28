//! Predefined actions. These serve as targets for keybindings.

use crate::OxWM;

pub(crate) type Action<Conn> = fn(&mut OxWM<Conn>) -> crate::Result<()>;

pub(crate) fn quit<Conn>(oxwm: &mut OxWM<Conn>) -> crate::Result<()> {
    oxwm.keep_going = false;
    Ok(())
}
