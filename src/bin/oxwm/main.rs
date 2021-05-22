use oxwm::{Client, Clients};

mod action;
mod config;
use config::Config;
mod util;
use util::*;

use std::error::Error;
use std::process::Command;

use x11rb::connection::Connection;
use x11rb::protocol::xproto;
use x11rb::protocol::xproto::ConnectionExt;
use x11rb::protocol::Event;

/// General-purpose result type. Not very precise, but we're not actually doing
/// anything with errors other than letting them bubble up to the user, so this
/// is fine for now.
type Result<T> = std::result::Result<T, Box<dyn Error>>;

pub struct OxWM<Conn> {
    /// The source of all our problems.
    conn: Conn,
    /// The screen we're connected on.
    screen: xproto::Screen,
    /// Configuration data.
    config: Config<Conn>,
    /// Local client data.
    clients: Clients,
    /// "Keep going" flag. If this is set to `false` at the start of the event
    /// loop, the window manager will stop running.
    keep_going: bool,
    /// If a window is being dragged, then that state is stored here.
    drag: Option<Drag>,
}

/// The state of a window drag.
struct Drag {
    /// The window that is being dragged.
    window: xproto::Window,
    /// The x-position of the pointer relative to the window.
    x: i16,
    /// The y-position of the pointer relative to the window.
    y: i16,
}

impl<Conn> OxWM<Conn> {
    fn new(conn: Conn, screen: usize) -> Result<OxWM<Conn>>
    where
        Conn: Connection,
    {
        // Unfortunately, we can't acquire a connection here; we have to accept
        // one as an argument. Why? Because `x11rb::connect` returns an
        // existential `Connection`, but `Conn` is universally quantified.
        let setup = conn.setup();
        let screen = setup.roots[screen].clone();
        // Load the config file. (Do this first, since it's the most likely
        // thing to fail in normal usage scenarios.)
        log::debug!("Loading config file.");
        let config = Config::load()?;
        // Create the OxWM object.
        let mut ret = OxWM {
            conn,
            screen,
            config,
            clients: Clients::new(),
            keep_going: true,
            drag: None,
        };
        // Try to redirect structure events from children of the root window.
        // Only one client---which must be the WM, essentially by
        // definition---can do this; so if we fail here, another WM is probably
        // running.
        //
        // (Also, listen for other events we care about.)
        log::debug!("Selecting SUBSTRUCTURE_REDIRECT on the root window.");
        let root = ret.screen.root;
        ret.conn
            .change_window_attributes(
                root,
                &xproto::ChangeWindowAttributesAux::new().event_mask(
                    xproto::EventMask::PROPERTY_CHANGE
                        | xproto::EventMask::SUBSTRUCTURE_NOTIFY
                        | xproto::EventMask::SUBSTRUCTURE_REDIRECT,
                ),
            )?
            .check()?;
        // Adopt already-existing windows.
        log::debug!("Adopting windows.");
        ret.adopt_children(root)?;
        // Run startup programs.
        log::debug!("Running startup programs.");
        for program in &ret.config.startup {
            if let Err(err) = Command::new(program).spawn() {
                log::warn!("Unable to execute startup program `{}': {:?}", program, err);
            }
        }
        // Get a passive grab on all bound keycodes.
        log::debug!("Grabbing bound keycodes.");
        ret.config
            .keybinds
            .keys()
            .map(|keycode| {
                ret.conn.grab_key(
                    false,
                    root,
                    ret.config.mod_mask,
                    *keycode,
                    xproto::GrabMode::ASYNC,
                    xproto::GrabMode::ASYNC,
                )
            })
            .collect::<Vec<_>>()
            .into_iter()
            .try_for_each(|cookie| cookie?.check())?;
        // Done.
        Ok(ret)
    }

    /// Run the WM. Note that this consumes the OxWM object: once
    /// this procedure returns, the connection to the X server is gone.
    fn run(mut self) -> Result<()>
    where
        Conn: Connection,
    {
        while self.keep_going {
            let ev = self.conn.wait_for_event()?;
            log::debug!("{:?}", ev);
            match ev {
                Event::ButtonPress(ev) => {
                    // We're only listening for button presses on button 1 with
                    // the modifier key down, so if we get a ButtonPress event,
                    // we start dragging.
                    if !ev.same_screen {
                        // TODO
                        log::error!("Don't know what to do when same_screen is false.");
                        continue;
                    }
                    if self.drag.is_some() {
                        log::error!("ButtonPress event during a drag.");
                        continue;
                    }
                    self.drag = Some(Drag {
                        window: ev.event,
                        x: ev.event_x,
                        y: ev.event_y,
                    })
                }
                Event::ButtonRelease(_) => match self.drag {
                    None => log::error!("ButtonRelease event without a drag."),
                    Some(_) => self.drag = None,
                },
                Event::CreateNotify(ev) => {
                    self.adopt_window(ev.window, ev.x, ev.y, ev.width, ev.height)?;
                }
                Event::ConfigureNotify(ev) => self
                    .clients
                    .configure(ev.window, ev.x, ev.y, ev.width, ev.height),
                Event::ConfigureRequest(ev) => {
                    self.conn
                        .configure_window(
                            ev.window,
                            &xproto::ConfigureWindowAux::from_configure_request(&ev),
                        )?
                        .check()?;
                }
                Event::DestroyNotify(ev) => self.clients.remove(ev.window),
                Event::KeyPress(ev) => {
                    // We're only listening for keycodes that are bound in the keybinds
                    // map (anything else is a bug), so we can call unwrap() with a
                    // clean conscience here.
                    let action = self.config.keybinds.get(&ev.detail).unwrap();
                    action(&mut self);
                }
                Event::MapRequest(ev) => {
                    self.conn.map_window(ev.window)?.check()?;
                }
                Event::MotionNotify(ev) => match self.drag {
                    None => log::error!("MotionNotify event without a drag."),
                    Some(ref drag) => {
                        let x = (ev.root_x - drag.x) as i32;
                        let y = (ev.root_y - drag.y) as i32;
                        self.conn
                            .configure_window(
                                drag.window,
                                &xproto::ConfigureWindowAux::new().x(x).y(y),
                            )?
                            .check()?;
                    }
                },
                _ => log::warn!("Unhandled event!"),
            }
        }
        Ok(())
    }

    /// Adopt all children of the given window.
    fn adopt_children(&mut self, root: xproto::Window) -> Result<()>
    where
        Conn: Connection,
    {
        let children = self.conn.query_tree(root)?.reply()?.children;
        self.adopt_windows(children.into_iter())?;
        Ok(())
    }

    /// Adopt every window in the provided iterator.
    fn adopt_windows<Iter>(&mut self, windows: Iter) -> Result<()>
    where
        Conn: Connection,
        Iter: Iterator<Item = xproto::Window>,
    {
        let conn = &self.conn;
        // Send some messages to the server. We send out all the messages before
        // checking any replies.
        let cookies = windows
            .map(|window| {
                (
                    window,
                    conn.get_window_attributes(window),
                    conn.get_geometry(window),
                    conn.get_property(
                        false,
                        window,
                        xproto::AtomEnum::WM_NAME,
                        xproto::AtomEnum::STRING,
                        0,
                        0,
                    ),
                    conn.grab_button(
                        false,
                        window,
                        event_mask_to_u16(
                            xproto::EventMask::BUTTON_PRESS
                                | xproto::EventMask::BUTTON_RELEASE
                                | xproto::EventMask::POINTER_MOTION,
                        ),
                        xproto::GrabMode::ASYNC,
                        xproto::GrabMode::ASYNC,
                        x11rb::NONE,
                        x11rb::NONE,
                        xproto::ButtonIndex::M1,
                        self.config.mod_mask,
                    ),
                )
            })
            .collect::<Vec<_>>();
        for (window, cookie1, cookie2, cookie3, cookie4) in cookies {
            // REVIEW Here is my reasoning for this code. If a cookie is an Err,
            // then there was a connection error, which is fatal. But if a
            // reply/check is an Err, that just means that the window is gone,
            // which shouldn't be fatal.
            match (
                cookie1?.reply(),
                cookie2?.reply(),
                cookie3?.reply(),
                cookie4?.check(),
            ) {
                (Ok(_), Ok(reply2), Ok(reply3), Ok(_)) => {
                    let name = String::from_utf8(reply3.value).unwrap();
                    self.clients.add(
                        window,
                        Client {
                            x: reply2.x,
                            y: reply2.y,
                            width: reply2.width,
                            height: reply2.height,
                            name,
                        },
                    );
                }
                _ => log::warn!("Something went wrong while adopting window {}.", window),
            }
        }
        Ok(())
    }

    /// Adopt a single window using some information that is already at hand.
    fn adopt_window(
        &mut self,
        window: xproto::Window,
        x: i16,
        y: i16,
        width: u16,
        height: u16,
    ) -> Result<()>
    where
        Conn: Connection,
    {
        log::debug!("Adopting window {}.", window);
        let cookie1 = self.conn.grab_button(
            false,
            window,
            event_mask_to_u16(
                xproto::EventMask::BUTTON_PRESS
                    | xproto::EventMask::BUTTON_RELEASE
                    | xproto::EventMask::POINTER_MOTION,
            ),
            xproto::GrabMode::ASYNC,
            xproto::GrabMode::ASYNC,
            x11rb::NONE,
            x11rb::NONE,
            xproto::ButtonIndex::M1,
            self.config.mod_mask,
        );
        let cookie2 = self.conn.get_property(
            false,
            window,
            xproto::AtomEnum::WM_NAME,
            xproto::AtomEnum::STRING,
            0,
            0,
        );
        match (cookie1?.check(), cookie2?.reply()) {
            (Ok(_), Ok(reply2)) => {
                // TODO This is horribly wrong. X uses its own special text
                // encoding, called "compound text". We actually need to write a
                // decoder.
                //
                // I'm calling unwrap() here because we're already doing this
                // the wrong way.
                let name = String::from_utf8(reply2.value).unwrap();
                self.clients.add(
                    window,
                    Client {
                        x,
                        y,
                        width,
                        height,
                        name,
                    },
                );
            }
            _ => log::warn!("Something went wrong while adopting window {}.", window),
        }
        Ok(())
    }
}

fn run_wm() -> Result<()> {
    log::debug!("Connecting to the X server.");
    let (conn, screen) = x11rb::connect(None)?;
    log::info!("Connected on screen {}.", screen);
    log::debug!("Initializing OxWM.");
    let oxwm = OxWM::new(conn, screen)?;
    log::debug!("Running OxWM.");
    oxwm.run()
}

fn main() -> Result<()> {
    simple_logger::SimpleLogger::new().init()?;
    run_wm()
}
