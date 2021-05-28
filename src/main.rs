mod action;
mod config;
mod ext;
mod util;

use std::collections::HashMap;
use std::error::Error;
use std::process::Command;

use x11rb::connection::Connection;
use x11rb::protocol::xproto;
use x11rb::protocol::xproto::ConfigureWindowAux;
use x11rb::protocol::xproto::ConnectionExt;
use x11rb::protocol::Event::*;

use config::Config;
use ext::conn::OxConnectionExt;
use util::*;

/// General-purpose result type. Not very precise, but we're not actually doing
/// anything with errors other than letting them bubble up to the user, so this
/// is fine for now.
type Result<T> = std::result::Result<T, Box<dyn Error>>;

pub(crate) struct OxWM<Conn> {
    /// The source of all our problems.
    conn: Conn,
    /// The index of the screen we're connected on.
    screen: usize,
    /// Configuration data.
    config: Config<Conn>,
    /// Local client data.
    clients: HashMap<xproto::Window, Client>,
    /// The currently-focused window.
    focus: Option<xproto::Window>,
    /// "Keep going" flag. If this is set to `false` at the start of the event
    /// loop, the window manager will stop running.
    keep_going: bool,
    /// If a window is being dragged, then that state is stored here.
    drag: Option<Drag>,
}

impl<Conn> OxWM<Conn> {
    /// Initialize the window manager.
    fn new(conn: Conn, screen: usize) -> Result<OxWM<Conn>>
    where
        Conn: Connection,
    {
        // Unfortunately, we can't acquire a connection here; we have to accept
        // one as an argument. Why? Because `x11rb::connect` returns an
        // existential `Connection`, but `Conn` is universally quantified.
        log::debug!("Loading config file.");
        // Load the config file first, since this is where errors are most
        // likely to occur.
        let config = Config::load()?;
        let mut ret = OxWM {
            conn,
            screen,
            config,
            clients: HashMap::new(),
            focus: None,
            keep_going: true,
            drag: None,
        };
        // Grab the server so that we can do setup atomically. We don't need to
        // worry about ungrabbing if we fail, because this function consumes the
        // connection.
        ret.conn.grab_server()?.check()?;
        ret.init()?;
        ret.conn.ungrab_server()?.check()?;
        Ok(ret)
    }

    /// Perform setup and initialization.
    fn init(&mut self) -> Result<()>
    where
        Conn: Connection,
    {
        // Try to become the window manager early, so that we can fail early
        // if necessary.
        self.become_wm()?;
        // Find and adopt extant clients.
        self.manage_extant()?;
        // General X setup.
        self.global_setup()?;
        // Run startup programs.
        self.run_startup_programs()?;
        // Done.
        Ok(())
    }

    /// Try to become the window manager.
    fn become_wm(&self) -> Result<()>
    where
        Conn: Connection,
    {
        log::debug!("Trying to become the window manager.");
        self.conn
            .change_window_attributes(
                self.root(),
                &xproto::ChangeWindowAttributesAux::new()
                    .event_mask(xproto::EventMask::SUBSTRUCTURE_REDIRECT),
            )?
            .check()?;
        Ok(())
    }

    /// Find extant clients and manage them.
    fn manage_extant(&mut self) -> Result<()>
    where
        Conn: Connection,
    {
        let children = self.conn.query_tree(self.root())?.reply()?.children;
        for window in children {
            self.manage(window)?;
        }
        Ok(())
    }

    /// Perform global setup operations that involve the server.
    fn global_setup(&self) -> Result<()>
    where
        Conn: Connection,
    {
        log::debug!("Setting event mask on the root window.");
        self.conn
            .change_window_attributes(
                self.root(),
                &xproto::ChangeWindowAttributesAux::new().event_mask(
                    xproto::EventMask::FOCUS_CHANGE
                        | xproto::EventMask::SUBSTRUCTURE_NOTIFY
                        | xproto::EventMask::SUBSTRUCTURE_REDIRECT,
                ),
            )?
            .check()?;
        log::debug!("Grabbing bound keycodes.");
        self.config
            .keybinds
            .keys()
            .map(|keycode| {
                self.conn.grab_key(
                    false,
                    self.root(),
                    self.config.mod_mask,
                    *keycode,
                    xproto::GrabMode::ASYNC,
                    xproto::GrabMode::ASYNC,
                )
            })
            .collect::<Vec<_>>()
            .into_iter()
            .try_for_each(|cookie| cookie?.check())?;
        Ok(())
    }

    /// Run configured startup programs.
    fn run_startup_programs(&self) -> Result<()> {
        log::debug!("Running startup programs.");
        for program in &self.config.startup {
            if let Err(err) = Command::new(program).spawn() {
                log::warn!("Unable to execute startup program `{}': {:?}", program, err);
            }
        }
        Ok(())
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
                ButtonPress(ev) => {
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
                ButtonRelease(_) => match self.drag {
                    None => log::error!("ButtonRelease event without a drag."),
                    Some(_) => self.drag = None,
                },
                ConfigureNotify(ev) => match self.clients.get_mut(&ev.window) {
                    None => log::warn!("Window is not managed."),
                    Some(client) => {
                        client.x = ev.x;
                        client.y = ev.y;
                        client.width = ev.width;
                        client.height = ev.height;
                    }
                },
                ConfigureRequest(ev) => {
                    self.conn
                        .configure_window(
                            ev.window,
                            &xproto::ConfigureWindowAux::from_configure_request(&ev),
                        )?
                        .check()?;
                }
                DestroyNotify(ev) => {
                    self.clients.remove(&ev.window);
                }
                FocusIn(ev) => self.focus = Some(ev.event),
                FocusOut(_) => self.focus = None,
                KeyPress(ev) => {
                    // We're only listening for keycodes that are bound in the keybinds
                    // map (anything else is a bug), so we can call unwrap() with a
                    // clean conscience here.
                    let action = self.config.keybinds.get(&ev.detail).unwrap();
                    action(&mut self)?;
                }
                MapRequest(ev) => {
                    self.manage(ev.window)?;
                    self.conn.map_window(ev.window)?.check()?;
                }
                MotionNotify(ev) => match self.drag {
                    None => log::error!("MotionNotify event without a drag."),
                    Some(ref drag) => {
                        let x = (ev.root_x - drag.x) as i32;
                        let y = (ev.root_y - drag.y) as i32;
                        self.conn
                            .configure_window(drag.window, &ConfigureWindowAux::new().x(x).y(y))?
                            .check()?;
                    }
                },
                _ => log::warn!("Unhandled event!"),
            }
        }
        Ok(())
    }

    /// Begin managing a window.
    fn manage(&mut self, window: xproto::Window) -> Result<()>
    where
        Conn: Connection,
    {
        log::debug!("Managing window {}.", window);
        // [Send out requests...
        let cookie1 = self.conn.get_window_attributes(window);
        let cookie2 = self.conn.get_geometry(window);
        // Get WM_NAME.
        let cookie3 = self.conn.get_property_simple(
            window,
            xproto::AtomEnum::WM_NAME,
            xproto::AtomEnum::STRING,
        );
        // Grab modifier+M1-press.
        let cookie4 = self.conn.grab_button(
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
        // Set our desired event mask.
        let cookie5 = self.conn.change_window_attributes(
            window,
            &xproto::ChangeWindowAttributesAux::new()
                .event_mask(xproto::EventMask::FOCUS_CHANGE | xproto::EventMask::PROPERTY_CHANGE),
        );
        // ...and put everything together at the end.]
        match (
            cookie1?.reply(),
            cookie2?.reply(),
            cookie3?.reply(),
            cookie4?.check(),
            cookie5?.check(),
        ) {
            (Ok(attrs), Ok(geom), Ok(prop_wm_name), Ok(_), Ok(_)) => {
                if attrs.override_redirect {
                    log::debug!("Ignoring window with override-redirect set.");
                } else {
                    // TODO Implement compound text decoding.
                    let name = String::from_utf8(prop_wm_name.value).unwrap();
                    log::debug!("Window name: {}.", name);
                    self.clients.insert(
                        window,
                        Client {
                            x: geom.x,
                            y: geom.y,
                            width: geom.width,
                            height: geom.height,
                            name,
                        },
                    );
                }
            }
            _ => log::warn!("Error while trying to manage the window."),
        }
        Ok(())
    }

    /// Kill the currently-focused client.
    fn kill_focused_client(&self) -> Result<()>
    where
        Conn: Connection,
    {
        log::debug!("Destroying focused window.");
        match self.focus {
            None => log::debug!("No focused window."),
            Some(window) => self.conn.kill_client(window)?.check()?,
        }
        Ok(())
    }

    /// Get the root window.
    fn root(&self) -> xproto::Window
    where
        Conn: Connection,
    {
        self.conn.setup().roots[self.screen].root
    }
}

#[derive(Debug)]
struct Client {
    x: i16,
    y: i16,
    width: u16,
    height: u16,
    name: String,
}

/// The state of a window drag.
#[derive(Debug)]
struct Drag {
    /// The window that is being dragged.
    window: xproto::Window,
    /// The x-position of the pointer relative to the window.
    x: i16,
    /// The y-position of the pointer relative to the window.
    y: i16,
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
