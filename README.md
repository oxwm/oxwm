# OxWM: An X11 window manager in Rust

Authors: Nicholas Coltharp, James Nichols

## Description

OxWM is a small window manager for X11.

X11 is (for a little while longer) the standard display system for most Unix and
Unix-y systems, including Linux. X11 uses an asynchronous client-server model. A
single program, the /X server/, lets client programs connect and make requests;
e.g., configuring and displaying windows. Although clients most commonly connect
over a Unix domain socket on the same machine, this is not a requirement;
indeed, it is possible for a client to connect to a server over a network
interface, allowing a program to run on one computer while displaying on another.

One of the major design decisions of X11 protocol is that it provides
"mechanism, not policy". That is, while the protocol specifies what kinds of
requests may be made of the server and what events clients may be notified of,
it does not specify any particular UI model. For example, a client may listen
for a mouse click event on one of its windows, but there is no guarantee that
the window will be on top after receiving this event.

Policy is instead provided by a _window manager_. This is a client that is
granted special privileges by the server, allowing it to intercept certain
requests made by other clients and grant, modify, or deny them. To put this in
concrete terms, a window manager is typically responsible for things like:

- Drawing borders and title bars around windows
- Allowing users to move and resize windows
- Allowing users to minimize and close windows
- Providing keyboard shortcuts for managing windows; e.g., Alt-Tab

Although OxWM doesn't provide all of this functionality, I hope this gives a
general idea of the task it needs to accomplish.

X11 window managers also generally try to comply with the _Inter-Client
Communication Conventions Manual_ (ICCCM), a set of standards specifying how X
clients should interact with each other. This provides for things like, e.g.,
letting a window manager know a window's title, or its preferred dimensions.

## Getting Started

You might want to have a configuration file. This is a file named
`oxwm/config.toml` in your platform-specific configuration directory. (For
Linux, this is usually `~/.config/`; i.e., your config file should be
`~/.config/oxwm/config.toml`.) A sensible default config file might look like
this:

```toml

mod_mask = "mod4"  # Super/Windows key
startup = ["konsole"]
focus_model = "click"  # or "autofocus" for focus-follows-mouse

[keybinds]
Escape = "quit"
q = "kill"
```

This tells OxWM that `mod4` is the global modifier key, that OxWM should run
`konsole` on startup, and that it should use a click-to-focus model. It also
tells OxWM that, with the modifier key pressed, pressing the `Escape` key should
exit, and pressing `q` should immediately abort the process of the focused
window.

If you don't create a config file, one will be generated for you.

After you've configured the program, you'll want to make your `~/.xinitrc` look
something like this:

```sh
xsetroot -cursor_name left_ptr &
pushd $OXWM_DIRECTORY  # wherever you've cloned the source to
cargo build && cargo install --path .
popd
exec oxwm >~/.Xoutput 2>&1  # assuming Cargo's bin/ directory is in your $PATH
```

Then, from a TTY, just type `startx`.

You'll want to make sure to have a terminal emulator in your list of startup
programs---otherwise, you won't have any way to run new programs.

OxWM is not (currently) a reparenting WM, so you'll need to use your "kill"
binding to close windows. You can drag windows around with mod+left mouse, and
you can resize windows with mod+right mouse.

We don't have full ICCCM compliance, but we have at least partial support for
all of the following:

- WM_PROTOCOLS
- WM_STATE
- WM_SIZE_HINTS

Note: we currently log every single event we receive, which can seriously impact
performance (you'll probably notice it when dragging windows). You can probably
improve performance by redirecting the log to `/dev/null`, or by just letting it
go to stdout.

## Testing

Due to the nature of the program, very little automated testing is possible (as
far as we know). In theory, it should be possible to create a "mock" X
connection and drive it programmatically, but this would be a substantial
undertaking.

Instead, we had to resort to interactive testing: starting an X session, making
some windows, interacting with things, and querying windows via `xprop`.

## Future directions

In its current state, this is essentially a toy project, so there's lots of room
for expansion. However, we're unsatisfied with the codebase: it's quite messy
and not very DRY. We've spent a lot of time thinking about how to design better
fundamental abstractions, and fixing things will probably require a rewrite.
This is not a crazy suggestion, since we didn't know anything about the X
protocol when we started; knowing much more now, it should be easier to create a
sound design from the start.
