- Only a pseudonymisation domain's own consumer reads its pseudonyms
  through PIXm ITI-83 (#339). Another Consumer that names the domain as
  `targetSystem` now gets no pseudonym for it, where it read a minted one
  before, and a query naming no `targetSystem` carries the pseudonyms of
  the caller's own domains alone, by `GET` and by `POST`. A Consumer that
  read another domain's pseudonyms must ask as that domain's consumer.
