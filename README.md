# Getting Started

You'll want to have a file named `oxwm/config.toml` in your platform-specific
configuration directory. For Linux, this is usually `~/.config/`. My config file
looks like this:

```toml

mod_mask = "mod4"
startup = ["konsole"]

[keybinds]
Escape = "quit"
q = "kill"
```

This tells OxWM that `mod4` (usually Super/the Windows key) is the global
modifier key, and that OxWM should run `konsole` on startup. It also tells OxWM
that, with the modifier key pressed, pressing the `escape` key should exit, or 
pressing the `q` kill should immediately abort the process of the focused window.
You can use `xev` to determine the name for a particular key on your system.

Note that, at the moment, if you don't have this config file, the program just
won't run.

After you've configured the program, you'll want to make your `/.xinitrc` look
something like this:

```sh
xsetroot -cursor_name left_ptr &
pushd $OXWM_DIRECTORY # wherever you've cloned the source to
cargo install --path .
popd
exec oxwm >~/.Xoutput 2>&1
```

Then, from a TTY, just type `startx`.

Note: we currently log every single event we receive, which can seriously impact
performance. You can probably improve performance by simply not redirecting the
log to a file.
