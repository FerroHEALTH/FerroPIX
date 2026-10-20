<!-- SPDX-FileCopyrightText: Cadasto B.V. -->
<!-- SPDX-License-Identifier: BUSL-1.1 -->

# Memory index

- [Memory lives in the repo](memory-lives-in-repo.md): every learning is a
  tracked file here, never a per-user note
- [What FerroPIX is](product-scope.md): the family's Master Patient Index,
  owning `ihe-iti` and `nl-generic-functions`, FHIR and HL7 v2 together;
  v0.0.1 setup, v0.0.2 the move, v0.0.3 the server; owner 2026-10-20
- [Built in FerroFED first, then moved](build-in-fed-first.md): the identity
  protocols were built in FerroFED, shaped to move; the move is v0.0.2, one
  step per crate with history (FerroFED#367)
- [Corpora move with the crates](corpora-move-with-crates.md): every corpus
  the moved crates read comes here with its script and provenance; each
  FerroFED swap PR removes the copies nothing there reads; owner 2026-10-20
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
- [PR licence box](pr-licence-box.md): tick the contribution-licence box in
  every pull request body; the owner's standing consent
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
- [FerroUI lockstep](ferroui-lockstep.md): a console, if one is ever added,
  is built on FerroUI and moves in lockstep with the others
