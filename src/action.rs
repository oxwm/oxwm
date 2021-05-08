//! Predefined actions. These serve as targets for keybindings.

use crate::OxWM;

pub type Action<Conn> = fn(&mut OxWM<Conn>);

pub fn quit<Conn>(oxwm: &mut OxWM<Conn>) {
    oxwm.keep_going = false;
}
