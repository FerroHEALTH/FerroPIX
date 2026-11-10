- The README's quick start clones the release tag (`git clone --branch
  v<version>`), and the book's quickstart page names the same clone, so the
  `quickstart.toml` a reader runs is the one the images `compose.yaml` pins
  read; a clone of `main` would pair a configuration key added there with a
  released server that refuses it (#619).
