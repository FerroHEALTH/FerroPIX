- The console's `[server] site_root` key, its
  `FERROPIX_CONSOLE__SERVER__SITE_ROOT` variable and the `/site` directory of
  the console image: there is no site bundle to serve. A configuration that
  still sets the key is refused at start, naming it, so remove it; the Helm
  chart (0.3.5) no longer sets it. The console's own single-key shortcuts
  (`l`, `n`, `d`, `r`, `[` and `]` on a pair, `o` and `d` on the review
  queue) are gone until FerroUI binds a control's key (#390); every action
  keeps its button or link, and `/`, `?`, `j`, `k`, Enter and the digits
  stay.
