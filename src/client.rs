use x11rb::connection::Connection;
use x11rb::protocol::xproto;
use x11rb::protocol::xproto::ConnectionExt;

use crate::Result;

/// Local data about a top-level window.
#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Debug)]
pub(crate) struct Client {
    /// The client window.
    pub(crate) window: xproto::Window,
    /// The client's state. We don't keep track of this for windows with
    /// override-redirect set.
    pub(crate) state: Option<ClientState>,
}

impl Client {
    /// Indicates whether a window is managed---that is, whether its
    /// override-redirect flag is not set.
    pub(crate) fn is_managed(&self) -> bool {
        self.state.is_some()
    }
}

#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Debug)]
/// Local data about the state of a top-level window.
pub(crate) struct ClientState {
    /// Horizontal position.
    pub(crate) x: i16,
    /// Vertical position.
    pub(crate) y: i16,
    /// Horizontal extent.
    pub(crate) width: u16,
    /// Vertical extent.
    pub(crate) height: u16,
    /// Whether the window is currently viewable.
    pub(crate) is_viewable: bool,
}

/// Local data about the state of all top-level windows. This includes windows
/// that have the override-redirect flag set, although we don't do anything
/// other than keep track of their stacking order and whether they're focused.
///
/// You can essentially think of this as a simulator for a tiny subset of the X
/// protocol. The point is that keeping track of the state of the server lets us
/// use our own knowledge when making decisions, rather than having to
/// constantly issue queries.
///
/// The user is responsible for the following.
/// * It is an error to try to insert two clients with the same window ID.
/// * It is an error to try to perform an operation on a window ID for which
///   there is no corresponding client.
#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Debug)]
pub(crate) struct Clients {
    // It feels wrong to use a vector, since we're going to be inserting and
    // removing elements---but Bjarne Stroustrup says that it takes a
    // ridiculously large n for linked lists to be faster than vectors (and
    // Rust's standard linked list type doesn't even have insert/remove
    // methods).
    stack: Vec<Client>,
    focus: Option<xproto::Window>,
}

impl Clients {
    /// Get the currently-focused client.
    pub(crate) fn get_focus(&self) -> Option<&Client> {
        let window = self.focus?;
        //DEBUG
            log::info!("---!--- Get focus found id {} ---!---", window); 
        //END DEBUG
        Some(self.get(window))
    }

    /// Get the currently-focused client.
    pub(crate) fn get_focus_mut(&mut self) -> Option<&mut Client> {
        let window = self.focus?;
        Some(self.get_mut(window))
    }

    /// Set the currently-focused client.
    pub(crate) fn set_focus<A>(&mut self, window: A)
    where
        A: Into<Option<xproto::Window>>,
    {
        let window = window.into();
        debug_assert!(window
            .map(|w| self.stack.iter().any(|c| c.window == w))
            .unwrap_or(true));
        //DEBUG
            log::info!("---!--- Clients now focusing: {:?}", window);
        //END DEBUG
        self.focus = window;

    }

    /// Get a client by its window.
    pub(crate) fn get(&self, window: xproto::Window) -> &Client {
        self.get_with_index(window).1
    }

    /// Get a client by its window.
    pub(crate) fn get_mut(&mut self, window: xproto::Window) -> &mut Client {
        self.get_with_index_mut(window).1
    }

    /// Get an iterator over the stack, from bottom to top.
    pub(crate) fn iter<'a>(&'a self) -> impl DoubleEndedIterator<Item = &'a Client> {
        self.stack.iter()
    }

    /// Get an iterator over the stack, from bottom to top.
    pub(crate) fn iter_mut<'a>(&'a mut self) -> impl DoubleEndedIterator<Item = &'a mut Client> {
        self.stack.iter_mut()
    }

    /// Initialize a new client stack by issuing queries to the server.
    pub(crate) fn new<Conn>(conn: &Conn, screen: usize) -> Result<Self>
    where
        Conn: Connection,
    {
        let root = conn.setup().roots[screen].root;
        let mut stack = Vec::new();
        let children = conn.query_tree(root)?.reply()?.children;
        for window in children {
            let attrs = conn.get_window_attributes(window)?.reply()?;
            let state = if attrs.override_redirect {
                None
            } else {
                let geom = conn.get_geometry(window)?.reply()?;
                Some(ClientState {
                    x: geom.x,
                    y: geom.y,
                    width: geom.width,
                    height: geom.height,
                    is_viewable: attrs.map_state == xproto::MapState::VIEWABLE,
                })
            };
            stack.push(Client { window, state })
        }
        let focus = conn.get_input_focus()?.reply()?.focus;
        let focus = if focus == x11rb::NONE || focus == root {
            None
        } else {
            Some(focus)
        };
        Ok(Clients { stack, focus })
    }

    /// Push a client on top of the stack.
    pub(crate) fn push(&mut self, client: Client) {
        debug_assert!(!self.stack.iter().any(|c| c.window == client.window));
        self.stack.push(client);
    }

    /// Remove a client from the stack.
    pub(crate) fn remove(&mut self, window: xproto::Window) -> Option<&Client> {
        
        let (i, _) = self.get_with_index(window);
        self.stack.remove(self.get_with_index(window).0);

        let next_focus = if self.focus? == window {
            // Removing the currently focused window.
            // Focus the first visible managed client that we can
            // find. 
            //

            {
            self.stack.iter()
                                 .rev()
                                 .skip(1)
                                 .find(|c| {
                    if let Some(ref st) = c.state {
                        if st.is_viewable && c.window != window{
                            return true
                        }
                    }

                    return false

                })
            } 
        }
        else
        {
            None
        };
        next_focus
    }

    /// Move a client to just above another one.
    pub(crate) fn to_above(&mut self, window: xproto::Window, sibling: xproto::Window) {
        let (i, _) = self.get_with_index(window);
        if i > 0 && self.stack[i - 1].window == sibling {
            return;
        }
        let client = self.stack.remove(i);
        let (j, _) = self.get_with_index(sibling);
        self.stack.insert(j + 1, client);
    }

    /// Lower a client to the bottom of the stack.
    pub(crate) fn to_bottom(&mut self, window: xproto::Window) {
        if self.stack.first().unwrap().window == window {
            return;
        }
        let (i, _) = self.get_with_index(window);
        let client = self.stack.remove(i);
        self.stack.insert(0, client);
    }

    /// Raise a client to the top of the stack.
    pub(crate) fn to_top(&mut self, window: xproto::Window) {
        if self.top().window == window {
            return;
        }
        let (i, _) = self.get_with_index(window);
        let client = self.stack.remove(i);
        self.stack.push(client)
    }

    /// Get the client that is on the top of the stack.
    pub(crate) fn top(&self) -> &Client {
        self.stack.last().unwrap()
    }

    /// Get the client that is on the top of the stack.
    pub(crate) fn top_mut(&mut self) -> &mut Client {
        self.stack.last_mut().unwrap()
    }

    // Private methods

    fn get_with_index(&self, window: xproto::Window) -> (usize, &Client) {
        self.iter()
            .enumerate()
            .find(|(_, c)| c.window == window)
            .unwrap()
    }

    fn get_with_index_mut(&mut self, window: xproto::Window) -> (usize, &mut Client) {
        self.iter_mut()
            .enumerate()
            .find(|(_, c)| c.window == window)
            .unwrap()
    }
}
