# OxWM

OxWM is the Oxidized Window Manager, a minimal X11 window manager written in Rust.

# Getting Started

You might want to have a file named `oxwm/config.toml` in your platform-specific
configuration directory. For Linux, this is usually `~/.config/`. A sensible
default config file might look like this:

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
exit, and pressing `q` should close the focused window, or immediately abort the
process of the focused window if it cannot be closed.

If you don't create this config file, one will be generated for you at the
appropriate location.

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

Note: we currently log every single event we receive, which can seriously impact
performance (you'll probably notice it when dragging windows). You can probably
improve performance by redirecting the log to `/dev/null`, or by just ignoring
it.

# Testing

Due to the nature of the program, very little automated testing is possible (as
far as we know). In theory, it should be possible to create a "mock" X
connection and drive it programmatically, but this would be a substantial
undertaking. Unit tests, as far as are possible without a functioning X connection,
have been implemented for the `Clients` and `Config` types.

# Future directions

In its current state, this is essentially a toy project, so there's lots of room
for expansion. However, we're unsatisfied with the codebase: it's quite messy.
We've spent a lot of time thinking about how to design better fundamental
abstractions, and fixing things will probably require a rewrite. This is not a
crazy suggestion, since we didn't know anything about the X protocol when we
started; knowing much more now, it should be easier to create a sound design
from the start.

# Authors

* Nicholas Coltharp
* James Nichols
