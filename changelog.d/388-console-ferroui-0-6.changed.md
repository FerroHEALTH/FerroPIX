- The operator console moves to FerroUI 0.6.0 and draws every page whole on
  its server: a link, a search, a filter or a decision loads one complete
  page, and FerroUI's small script modules add the keyboard, the side panels,
  the dialogs and the session warning on top, with every page working when
  they do not load. The console runs no WebAssembly and links no Leptos
  crate; its binary serves FerroUI's stylesheet, theme and script modules
  under `/ferrohealth-ui/0.6.0/`, cached for good, so the image carries the
  binary alone. The Content-Security-Policy now admits scripts and styles
  from the console's own origin alone, `script-src 'self'; style-src 'self'`,
  with no nonce and no `'wasm-unsafe-eval'`. The review queue's At a glance
  button now opens its side panel on a click, where the row's link took the
  press before.
