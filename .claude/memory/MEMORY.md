<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# Memory index

- [Family architecture](family-architecture.md): the FerroHEALTH family
  is five separate servers that reach each other only over published
  protocols, plus shared crates for cross-cutting work; nothing is
  hand-written that a shared crate or another product provides; owner
  2026-10-25
- [Memory lives in the repo](memory-lives-in-repo.md): every learning is a
  tracked file here, never a per-user note
- [What FerroPIX is](product-scope.md): the family's Master Patient Index
  and the one product that owns identity and the IHE profiles, owning
  `ihe-iti` and `nl-generic-functions`, FHIR and HL7 v2 together; owner
  2026-10-20 and 2026-10-25
- [Built in FerroFED first, then moved](build-in-fed-first.md): history; the
  identity protocols moved here in v0.0.2 (FerroFED#367) and the rule is
  retired, since identity is FerroPIX's lane alone
- [Corpora move with the crates](corpora-move-with-crates.md): every corpus
  the moved crates read comes here with its script and provenance; each
  FerroFED swap PR removes the copies nothing there reads; owner 2026-10-20
- [ihe-iti audit feature](ihe-iti-audit-feature.md): each profile's audit
  record stays in ihe-iti behind `balp` and `atna`, on ihe-atna's trail and
  user model
- [Licence: BUSL 1.1](license-busl.md): the FerroHEALTH terms, Cadasto B.V.
  the Licensor, Apache 2.0 four years after each version
- [No AI attribution](no-ai-attribution.md): never, anywhere; the hook
  blocks it
- [Support policy](support-policy.md): only the newest release, with the CRA
  five-year fix commitment
- [Signed commits and the merge queue](merge-queue-signed.md): `main` takes
  only signed commits through pull requests and the merge queue
- [Auto-merge](pr-auto-merge.md): arm every pull request with
  `gh pr merge <n> --auto`
- [Two version lines](two-version-lines.md): the product version is the
  workspace and the tag; each published crate keeps its own line
- [Release tag](release-tag.md): signed on the merge commit, pushed from the
  session; the crates-io environment waits for the owner
- [Release closes milestone](release-closes-milestone.md): release.yml
  refuses a tag whose milestone is not empty and closes it after every leg
- [Worker scratch subfolders](worker-scratch-subfolders.md): scratch files
  under the worker's own subfolder; re-read every PR body after setting it
- [Remove worktrees after merge](remove-worktrees-after-merge.md): every
  checkout under `.claude/worktrees/`, removed with `cargo clean` once merged,
  in every family repository
- [Worker wait loops](worker-wait-loops.md): no self-matching `pgrep -f`;
  poll at most every 60 seconds
- [FerroUI lockstep](ferroui-lockstep.md): the operator console
  `ferropix-console` (owner 2026-10-21, built on FerroUI 0.3.2 in v0.0.3) is
  in the library's FerroPIX brand, reactive like every console, moves in
  lockstep with the others, and every gap is filed and built in FerroUI
- [The book stays current](book-same-pr.md): ferropix.eu documents only what
  the code does, changes in the same PR, links the tracker for the planned,
  never types a moving version; owner 2026-10-21
- [Helm and Kubernetes in Podman](helm-kubernetes-in-podman.md): owner 2026-10-25, test charts locally on kind over Podman; install helm when missing; never build images locally
- [Reactive console UI](reactive-console-ui.md): superseded 2026-10-29 by FerroUI #510; every console is drawn on the server, a navigation is a document load, an in-page change a FerroUI module, no WebAssembly
