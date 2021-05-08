# TODO

* Make sure it works on Unix. I'm particularly concerned about the use of the
  `dirs` crate, since it doesn't explicitly mention any Unix (other than macOS)
  as a supported system.
* Possibly strengthen error types. Right now we just use `Box<dyn Error>`
  basically everywhere.
* Workspaces
* ICCCM compliance (this could be its own section)
