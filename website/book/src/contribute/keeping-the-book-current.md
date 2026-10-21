<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# Keeping this book current

This book describes only what the code does, so it changes with the code.

- **A change a user can see updates its book page in the same pull
  request**, as it adds its changelog entry: a crate feature, a client's
  behaviour, a specification pin, and, once the server lands, its
  configuration, its deployment files and its endpoints.
- **Planned work is named as planned** and links its issue or milestone on
  the tracker. A page never describes an unbuilt feature as present.
- **Versions are rendered, never typed.** The crate and product versions on
  these pages and the specification and toolchain tables come from the
  manifests and the pin matrix when the site is assembled, so they cannot go
  stale.
- **Every internal link and anchor resolves.** CI assembles the site the way
  GitHub Pages serves it and checks every link offline on each pull request.

What a check can catch is limited: a link check passes on a page whose every
claim is false. The rest is review, which is why the same-pull-request rule
carries the weight.
