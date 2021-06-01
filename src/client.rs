use x11rb::protocol::xproto;

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

/// An abstraction for managing local (cached) client state. This keeps track of
/// which clients are currently managed, as well as their stacking order.
///
/// The user is responsible for checking invariants. In particular, it is an
/// error to try to insert two clients with the same window ID, and it is an
/// error to try to perform an operation on a window ID for which there is no
/// corresponding client.
#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Debug)]
pub(crate) struct Clients {
    // It feels wrong to use a vector, since we're going to be inserting and
    // removing elements---but Bjarne Stroustrup says that it takes a
    // ridiculously large n for linked lists to be faster than vectors (and
    // Rust's standard linked list type doesn't even have insert/remove
    // methods).
    stack: Vec<Client>,
}

impl Clients {
    /// Get a client by its window.
    pub(crate) fn get(&self, window: xproto::Window) -> &Client {
        self.get_with_index(window).1
    }

    /// Get a client by its window.
    pub(crate) fn get_mut(&mut self, window: xproto::Window) -> &mut Client {
        self.get_with_index_mut(window).1
    }

    /// Determine whether the stack is empty.
    pub(crate) fn is_empty(&self) -> bool {
        self.stack.is_empty()
    }

    /// Get an iterator over the stack, from bottom to top.
    pub(crate) fn iter<'a>(&'a self) -> impl DoubleEndedIterator<Item = &'a Client> {
        self.stack.iter()
    }

    /// Get an iterator over the stack, from bottom to top.
    pub(crate) fn iter_mut<'a>(&'a mut self) -> impl DoubleEndedIterator<Item = &'a mut Client> {
        self.stack.iter_mut()
    }

    /// Create a new, empty client stack.
    pub(crate) fn new() -> Clients {
        Clients { stack: Vec::new() }
    }

    /// Push a client on top of the stack.
    pub(crate) fn push(&mut self, client: Client) {
        debug_assert!(!self.stack.iter().any(|c| c.window == client.window));
        self.stack.push(client);
    }

    /// Remove a client from the stack.
    pub(crate) fn remove(&mut self, window: xproto::Window) {
        self.stack.remove(self.get_with_index(window).0);
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

    /// Lower a client to the botton of the stack.
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
