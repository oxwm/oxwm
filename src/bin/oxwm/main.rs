use essrpc::transports::BincodeTransport;
use essrpc::RPCError;
use essrpc::RPCErrorKind;
use essrpc::RPCServer;

use oxwm::*;

// mod action;
// mod config;
// use config::Config;
use oxwm::*;
mod util;
use util::*;

use serde::Serialize;

use std::os::unix::net::UnixListener;
use std::process::Command;
use std::thread;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use std::{error::Error, sync::MutexGuard};

use x11rb::protocol::xproto;
use x11rb::protocol::xproto::ConnectionExt;
use x11rb::protocol::Event;
use x11rb::{connection::Connection, protocol::xproto::ConfigureWindowAux};

// pub struct OxWM<Conn> {
//     /// The screen we're connected on.
//     screen: xproto::Screen,
//     /// Configuration data.
//     config: Config<Conn>,
//     /// Local client data.
//     clients: HashMap<xproto::Window, Client>,
//     /// "Keep going" flag. If this is set to `false` at the start of the event
//     /// loop, the window manager will stop running.
//     keep_going: bool,
//     /// If a window is being dragged, then that state is stored here.
//     drag: Option<Drag>,
// }

// /// The state of a window drag.
// struct Drag {
//     /// The window that is being dragged.
//     window: xproto::Window,
//     /// The x-position of the pointer relative to the window.
//     x: i16,
//     /// The y-position of the pointer relative to the window.
//     y: i16,
// }

// impl<Conn> OxWM<Conn> {
//     fn new(conn: Conn, screen: usize) -> Result<OxWM<Conn>>
//     where
//         Conn: Connection,
//     {
//         // Unfortunately, we can't acquire a connection here; we have to accept
//         // one as an argument. Why? Because `x11rb::connect` returns an
//         // existential `Connection`, but `Conn` is universally quantified.
//         let setup = conn.setup();
//         let screen = setup.roots[screen].clone();
//         // Load the config file. (Do this first, since it's the most likely
//         // thing to fail in normal usage scenarios.)
//         log::debug!("Loading config file.");
//         let config = Config::load()?;
//         // Create the OxWM object.
//         let mut ret = OxWM {
//             conn,
//             screen,
//             config,
//             clients: HashMap::new(),
//             keep_going: true,
//             drag: None,
//         };
//         // Try to redirect structure events from children of the root window.
//         // Only one client---which must be the WM, essentially by
//         // definition---can do this; so if we fail here, another WM is probably
//         // running.
//         //
//         // (Also, listen for other events we care about.)
//         log::debug!("Selecting SUBSTRUCTURE_REDIRECT on the root window.");
//         let root = ret.screen.root;
//         ret.conn
//             .change_window_attributes(
//                 root,
//                 &xproto::ChangeWindowAttributesAux::new().event_mask(
//                     xproto::EventMask::PROPERTY_CHANGE
//                         | xproto::EventMask::SUBSTRUCTURE_NOTIFY
//                         | xproto::EventMask::SUBSTRUCTURE_REDIRECT,
//                 ),
//             )?
//             .check()?;
//         // Adopt already-existing windows.
//         log::debug!("Adopting windows.");
//         ret.adopt_children(root)?;
//         // Run startup programs.
//         log::debug!("Running startup programs.");
//         for program in &ret.config.startup {
//             if let Err(err) = Command::new(program).spawn() {
//                 log::warn!("Unable to execute startup program `{}': {:?}", program, err);
//             }
//         }
//         // Get a passive grab on all bound keycodes.
//         log::debug!("Grabbing bound keycodes.");
//         ret.config
//             .keybinds
//             .keys()
//             .map(|keycode| {
//                 ret.conn.grab_key(
//                     false,
//                     root,
//                     ret.config.mod_mask,
//                     *keycode,
//                     xproto::GrabMode::ASYNC,
//                     xproto::GrabMode::ASYNC,
//                 )
//             })
//             .collect::<Vec<_>>()
//             .into_iter()
//             .try_for_each(|cookie| cookie?.check())?;
//         // Done.
//         Ok(ret)
//     }

//     /// Run the WM. Note that this consumes the OxWM object: once
//     /// this procedure returns, the connection to the X server is gone.
//     fn run(mut self) -> Result<()>
//     where
//         Conn: Connection,
//     {
//         // while self.keep_going {
//         //     let ev = self.conn.wait_for_event()?;
//         //     log::debug!("{:?}", ev);
//         //     match ev {
//         //         Event::ButtonPress(ev) => {
//         //             // We're only listening for button presses on button 1 with
//         //             // the modifier key down, so if we get a ButtonPress event,
//         //             // we start dragging.
//         //             if !ev.same_screen {
//         //                 // TODO
//         //                 log::error!("Don't know what to do when same_screen is false.");
//         //                 continue;
//         //             }
//         //             if self.drag.is_some() {
//         //                 log::error!("ButtonPress event during a drag.");
//         //                 continue;
//         //             }
//         //             self.drag = Some(Drag {
//         //                 window: ev.event,
//         //                 x: ev.event_x,
//         //                 y: ev.event_y,
//         //             })
//         //         }
//         //         Event::ButtonRelease(_) => match self.drag {
//         //             None => log::error!("ButtonRelease event without a drag."),
//         //             Some(_) => self.drag = None,
//         //         },
//         //         Event::CreateNotify(ev) => {
//         //             self.adopt_window(ev.window, ev.x, ev.y, ev.width, ev.height)?;
//         //         }
//         //         Event::ConfigureNotify(ev) => match self.clients.get_mut(&ev.event) {
//         //             None => log::warn!("Window isn't registered."),
//         //             Some(client) => {
//         //                 client.x = ev.x;
//         //                 client.y = ev.y;
//         //                 client.width = ev.width;
//         //                 client.height = ev.height;
//         //             }
//         //         },
//         //         Event::ConfigureRequest(ev) => {
//         //             self.conn
//         //                 .configure_window(
//         //                     ev.window,
//         //                     &xproto::ConfigureWindowAux::from_configure_request(&ev),
//         //                 )?
//         //                 .check()?;
//         //         }
//         //         Event::DestroyNotify(ev) => {
//         //             if self.clients.remove(&ev.window).is_none() {
//         //                 log::warn!("Window wasn't registered.")
//         //             }
//         //         }
//         //         Event::KeyPress(ev) => {
//         //             // We're only listening for keycodes that are bound in the keybinds
//         //             // map (anything else is a bug), so we can call unwrap() with a
//         //             // clean conscience here.
//         //             let action = self.config.keybinds.get(&ev.detail).unwrap();
//         //             action(&mut self);
//         //         }
//         //         Event::MapRequest(ev) => {
//         //             self.conn.map_window(ev.window)?.check()?;
//         //         }
//         //         Event::MotionNotify(ev) => match self.drag {
//         //             None => log::error!("MotionNotify event without a drag."),
//         //             Some(ref drag) => {
//         //                 let x = (ev.root_x - drag.x) as i32;
//         //                 let y = (ev.root_y - drag.y) as i32;
//         //                 self.conn
//         //                     .configure_window(
//         //                         drag.window,
//         //                         &xproto::ConfigureWindowAux::new().x(x).y(y),
//         //                     )?
//         //                     .check()?;
//         //             }
//         //         },
//         //         _ => log::warn!("Unhandled event."),
//         //     }
//         // }
//         // Ok(())
//     }

//     /// Adopt all children of the given window.
//     fn adopt_children(&mut self, root: xproto::Window) -> Result<()>
//     where
//         Conn: Connection,
//     {
//         let children = self.conn.query_tree(root)?.reply()?.children;
//         self.adopt_windows(children.into_iter())?;
//         Ok(())
//     }

//     /// Adopt every window in the provided iterator.
//     fn adopt_windows<Iter>(&mut self, windows: Iter) -> Result<()>
//     where
//         Conn: Connection,
//         Iter: Iterator<Item = xproto::Window>,
//     {
//         let conn = &self.conn;
//         // Send some messages to the server. We send out all the messages before
//         // checking any replies.
//         let cookies = windows
//             .map(|window| {
//                 (
//                     window,
//                     conn.get_window_attributes(window),
//                     conn.get_geometry(window),
//                     conn.get_property(
//                         false,
//                         window,
//                         xproto::AtomEnum::WM_NAME,
//                         xproto::AtomEnum::STRING,
//                         0,
//                         0,
//                     ),
//                     conn.grab_button(
//                         false,
//                         window,
//                         event_mask_to_u16(
//                             xproto::EventMask::BUTTON_PRESS
//                                 | xproto::EventMask::BUTTON_RELEASE
//                                 | xproto::EventMask::POINTER_MOTION,
//                         ),
//                         xproto::GrabMode::ASYNC,
//                         xproto::GrabMode::ASYNC,
//                         x11rb::NONE,
//                         x11rb::NONE,
//                         xproto::ButtonIndex::M1,
//                         self.config.mod_mask,
//                     ),
//                 )
//             })
//             .collect::<Vec<_>>();
//         for (window, cookie1, cookie2, cookie3, cookie4) in cookies {
//             // REVIEW Here is my reasoning for this code. If a cookie is an Err,
//             // then there was a connection error, which is fatal. But if a
//             // reply/check is an Err, that just means that the window is gone,
//             // which shouldn't be fatal.
//             match (
//                 cookie1?.reply(),
//                 cookie2?.reply(),
//                 cookie3?.reply(),
//                 cookie4?.check(),
//             ) {
//                 (Ok(_), Ok(reply2), Ok(reply3), Ok(_)) => {
//                     // TODO Implement compound text decoding.
//                     let name = String::from_utf8(reply3.value).unwrap();
//                     self.clients.insert(
//                         window,
//                         Client {
//                             x: reply2.x,
//                             y: reply2.y,
//                             width: reply2.width,
//                             height: reply2.height,
//                             name,
//                         },
//                     );
//                 }
//                 _ => log::warn!("Something went wrong while adopting window {}.", window),
//             }
//         }
//         Ok(())
//     }

//     /// Adopt a single window using some information that is already at hand.
//     fn adopt_window(
//         &mut self,
//         window: xproto::Window,
//         x: i16,
//         y: i16,
//         width: u16,
//         height: u16,
//     ) -> Result<()>
//     where
//         Conn: Connection,
//     {
//         log::debug!("Adopting window {}.", window);
//         let cookie1 = self.conn.grab_button(
//             false,
//             window,
//             event_mask_to_u16(
//                 xproto::EventMask::BUTTON_PRESS
//                     | xproto::EventMask::BUTTON_RELEASE
//                     | xproto::EventMask::POINTER_MOTION,
//             ),
//             xproto::GrabMode::ASYNC,
//             xproto::GrabMode::ASYNC,
//             x11rb::NONE,
//             x11rb::NONE,
//             xproto::ButtonIndex::M1,
//             self.config.mod_mask,
//         );
//         let cookie2 = self.conn.get_property(
//             false,
//             window,
//             xproto::AtomEnum::WM_NAME,
//             xproto::AtomEnum::STRING,
//             0,
//             0,
//         );
//         match (cookie1?.check(), cookie2?.reply()) {
//             (Ok(_), Ok(reply2)) => {
//                 // TODO Implement compound text decoding.
//                 let name = String::from_utf8(reply2.value).unwrap();
//                 self.clients.insert(
//                     window,
//                     Client {
//                         x,
//                         y,
//                         width,
//                         height,
//                         name,
//                     },
//                 );
//             }
//             _ => log::warn!("Something went wrong while adopting window {}.", window),
//         }
//         Ok(())
//     }
// }

// fn run_wm() -> Result<()> {
//     log::debug!("Connecting to the X server.");
//     let (conn, screen) = x11rb::connect(None)?;
//     log::info!("Connected on screen {}.", screen);
//     log::debug!("Initializing OxWM.");
//     let oxwm = OxWM::new(conn, screen)?;
//     log::debug!("Running OxWM.");
//     oxwm.run()
// }
//

#[derive(Debug)]
pub struct OxWM<Conn> {
    conn: Conn,
    state: Mutex<OxWMState>,
}

impl<Conn> Ox for OxWM<Conn>
where
    Conn: Connection,
{
    fn ls(&self) -> std::result::Result<OxWMState, essrpc::RPCError> {
        Ok(self.state.lock().into_rpc_error()?.clone())
    }

    fn configure_window(
        &self,
        window: xproto::Window,
        x: Option<i32>,
        y: Option<i32>,
        width: Option<u32>,
        height: Option<u32>,
        border_width: Option<u32>,
        sibling: Option<xproto::Window>,
        stack_mode: Option<StackMode>,
    ) -> std::result::Result<(), RPCError> {
        let stack_mode = stack_mode.map(|m| xproto::StackMode::from(m));
        self.conn
            .configure_window(
                window,
                &xproto::ConfigureWindowAux {
                    x,
                    y,
                    width,
                    height,
                    border_width,
                    sibling,
                    stack_mode,
                },
            )
            .into_rpc_error()?
            .check()
            .into_rpc_error()
    }
}

fn main() -> Result<()> {
    let (conn, screen) = x11rb::connect(None)?;
    let root = conn.setup().roots[screen].root;
    // Grab the server during setup so that we can do everything atomically.
    let clients = with_grabbed_server(&conn, || -> Result<HashMap<xproto::Window, Client>> {
        let children = conn.query_tree(root)?.reply()?.children;
        let clients = children
            .into_iter()
            .map(|child| {
                (
                    child,
                    conn.get_geometry(child),
                    conn.get_property(
                        false,
                        child,
                        xproto::AtomEnum::WM_NAME,
                        xproto::AtomEnum::STRING,
                        0,
                        0,
                    ),
                )
            })
            .collect::<Vec<_>>()
            .into_iter()
            .map(|(child, cookie1, cookie2)| {
                let geom = cookie1?.reply()?;
                let name = cookie2?.reply()?;
                Ok((
                    child,
                    Client {
                        x: geom.x,
                        y: geom.y,
                        width: geom.width,
                        height: geom.height,
                        name: name.value,
                    },
                ))
            })
            // We've grabbed the server, so the only possible error is a
            // connection error, which should invalidate the whole operation.
            .collect::<Result<HashMap<_, _>>>()?;
        conn.change_window_attributes(
            root,
            &xproto::ChangeWindowAttributesAux::new()
                .event_mask(xproto::EventMask::SUBSTRUCTURE_NOTIFY),
        )?
        .check()?;
        Ok(clients)
    })?;

    let oxwm = Arc::new(OxWM {
        conn,
        state: Mutex::new(OxWMState { clients }),
    });

    // Spawn a thread to handle RPC.
    let server = UnixListener::bind("/tmp/oxwm")?;
    let oxwm_clone = oxwm.clone();
    thread::spawn(move || loop {
        let (sock, _) = server.accept().unwrap();
        OxRPCServer::new(oxwm_clone.clone(), BincodeTransport::new(sock))
            .serve_single_call()
            .unwrap();
    });
    loop {
        match oxwm.conn.wait_for_event()? {
            Event::CreateNotify(ev) =>
            // TODO .lock() is difficult. It returns a Result<T,
            // PoisonError<T>>; and since T occurs in the error type, Rust
            // (correctly) infers that the error must not outlive the thing
            // we're trying to acquire. This is incompatible with the
            // error-handling discipline we've adopted, so we're just calling
            // .unwrap() right now.
            //
            // (Sidenote: something about the way type parameters interact with
            // the lifetime checker bothers me. Like, what if PoisonError<T>
            // only contained a PhantomData<T>, not an actual T? Then the
            // lifetime constraint would be way too strict. Rust's assumption
            // that anything with a type parameter T has-a T is confusing, and
            // probably prevents the language from ever having real type-level
            // lambdas.)
            {
                let mut st = oxwm.state.lock().unwrap();
                let client = st.clients.get_mut(&ev.window).unwrap();
                client.x = ev.x;
                client.y = ev.y;
                client.width = ev.width;
                client.height = ev.height;
            }
            Event::ConfigureNotify(ev) => {
                let mut st = oxwm.state.lock().unwrap();
                let client = st.clients.get_mut(&ev.window).unwrap();
                client.x = ev.x;
                client.y = ev.y;
                client.width = ev.width;
                client.height = ev.height;
            }
            Event::DestroyNotify(ev) => {
                let mut st = oxwm.state.lock().unwrap();
                st.clients.remove(&ev.event);
            }
            _ => (),
        }
    }
    Ok(())
}

// fn receive_events<Conn>(
//     ev_send: crossbeam::channel::Sender<(
//         Conn,
//         std::result::Result<Event, x11rb::errors::ConnectionError>,
//     )>,
//     conn_recv: crossbeam::channel::Receiver<Conn>,
// ) -> !
// where
//     Conn: Connection + Send,
// {
//     loop {
//         let conn = conn_recv.recv().unwrap();
//         let event = conn.wait_for_event();
//         ev_send.send((conn, event)).unwrap();
//     }
// }

// fn handle_ipc<Conn>(
//     oxwm: Arc<OxWM<Conn>>,
//     ipc_recv: unix_ipc::Receiver<(unix_ipc::Sender<OxWMState>, Request)>,
// ) -> ! {
//     loop {
//         // TODO We probably shouldn't use unwrap() here; I think some other client
//         // application could send malformed messages, which would manifest as
//         // errors, which (as things stand) would crash the IPC thread.
//         let (ipc_send, req) = ipc_recv.recv().unwrap();
//         match req {
//             Request::Ls => {}
//         }
//     }
// }
