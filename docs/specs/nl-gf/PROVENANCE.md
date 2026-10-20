<!-- This file describes vendored third-party material; the bytes beside it
     keep their upstream licence, not the licence of this repository. -->

# Provenance: the Netherlands Generic Functions IG source

Vendored verbatim by `scripts/vendor/nl-gf.sh`. Never edit a file here:
change the pin in docs/VERSIONS.md and re-run the script.

- Source: <https://github.com/nuts-foundation/nl-generic-functions-ig>, rendered at
  <https://build.fhir.org/ig/nuts-foundation/nl-generic-functions-ig/>
- Pin: tag `v0.3.0`, which resolves to commit `5367430787042c218996f11570f904bd3cd37a83`
- Document: *Netherlands - Generic Functions for data exchange
  Implementation Guide*, package `fhir.nl.gf` version 0.3.0, published by
  Stichting Nuts
- Fetched: 2026-10-20
- Upstream licence: the European Union Public Licence 1.2 (`EUPL-1.2`, the
  `license` of `sushi-config.yaml` and the repository's `LICENSE`,
  vendored beside this file)
- FHIR version: 4.0.1
- Layout: the upstream paths, unchanged
- Files: 33
- Tree digest (sha256 over the sorted per-file `sha256  path` listing,
  `PROVENANCE.md` excluded): `2ea858fc732ae61932e582db9dab6d64d664d11caa15363f4ede65ae46ee5610`
- Read by: the NVI search of `crates/nl-generic-functions` feature
  `nvi`, held to the Localization Service capability statement and the
  localization record profile, the LRZa reading of feature `lrza`, held
  to the Organization profiles and the LRZa examples, and the access token
  request of feature `nuts-auth`, held to GF-Authentication and its
  GFI-004 and GFI-005 transactions

## What is taken

The IG has no package on the FHIR package registry; its release is the git
tag, so the source is taken. The narrative pages of the functions FerroFED's
Federation Tier specification binds (localization, consent, the care
services directory, identification, authentication with its six GFI
transactions, and authorization), the FSH sources of the profiles, naming
systems, code systems, value sets, search parameters and capability
statements those pages define, the examples of the localization record and of
the LRZa Administration Directory and the Query Directory, the localization
sequence diagram and the sequence diagrams of Request Access Token (GFI-004)
and Authenticated Interaction (GFI-005), and `sushi-config.yaml`, which
names the package, its version and its licence. The pages and examples of
routing, care teams and workflow, the rendered images and the build scripts
serve no reader here and are not taken.

| File | sha256 | git blob id |
|---|---|---|
| `LICENSE` | `942aaa9ff29282e01361b9ec87aeadb3b40a6131ea6d1c82bef554d3d7b02d75` | `fee48b081d43512ba8bc53022c5803f69cdab33c` |
| `input/fsh/aliases.fsh` | `d71499143c285923f9b332c7a6fa2db9a9c065d731f50a080f632df5431d0e08` | `8eef43cd507e1be8e2b59cc43085692a5cae1bc8` |
| `input/fsh/capabilitystatement-admindirectory-updateclient.fsh` | `4afc41f1206abef14dc7728dff6c6e988641936fb5bad125b8ddc120aaa688d8` | `b1a04e83dda32f08cddf08af54e4274e7bf36eed` |
| `input/fsh/capabilitystatement-localization-lmr.fsh` | `dafe5e24e0876ba44aede5bf14fa9e4a18b33010c93cec9d21ddd6d26119b415` | `6d8344f75288dae04a8da14d216d1f551b2b64c1` |
| `input/fsh/capabilitystatement-localization-repository.fsh` | `a7236ca6cb994cb2644aaede635333e98061fedab1097cc079f0c895855dc0c7` | `f556499e0e4cf9098e9146cd8045e10dba022221` |
| `input/fsh/capabilitystatement-querydirectory.fsh` | `ae6b0560608d3c43730c9342eb46db00fc1306baaa022cb6a41953c58a56aebf` | `3936bb3304c201806178a56ecbc5145db1a1a901` |
| `input/fsh/codesystems.fsh` | `8232554d7b3d3cc501a460faeb0623d99d1a1edb923ec4a072d84ee60aea826c` | `01b53f79a37e9b2f4a42ef8ffb477500faa5bc77` |
| `input/fsh/examples/admin-directory-lrza.fsh` | `e0544ff266e1b1658e7517984de522dd934499c8008abd29c19b5b3dc7e24e9a` | `1ed01483283dafa5ce0138ee98a9f455f84b9764` |
| `input/fsh/examples/gf-localization.fsh` | `61e95adb67cab2bf4f10c5367e7340a8e533349942569dd1138d9fdd128deeef` | `6c82740fa2a241b79efac5f41271c20a671b971f` |
| `input/fsh/examples/query-directory.fsh` | `8635ebc7651a0fde8134819f08eb650b21997c779ecc81255302006cfbd56e68` | `040d2f471935776e0303aa49b5c633c65bcadc1c` |
| `input/fsh/namingsystem.fsh` | `c7cef7eabcc43eb94cda1b955dfae211f59d3c3a7bc03bb2148310ced53d5b69` | `ff3f2cc4958ef6b9a3ae7fa336b9cd400a2ceab7` |
| `input/fsh/nl_gf_localization_document_reference.fsh` | `cff25f546f06fb248f0b738bc5448960896a586d5063bf8b54e535b4fb477398` | `c3a6ec6d3e52d33331c1289aec23bc4503a257ad` |
| `input/fsh/rulesets.fsh` | `cbb995f4ef1f57a0cb71165f602f42f0295e91e908d66416d681afa931366898` | `a140627da07d4205932abafa526b89cdfa0a41d1` |
| `input/fsh/searchparameters.fsh` | `cedb029f0021d74f0ee2396105a9138868712fa6a8efd6043b0c3ccf02aa97e5` | `08dae2c73c024e015a9fe826be910f72c8d3fb96` |
| `input/fsh/structuredefinitions.fsh` | `71c46c0eb180e2d3adaea6de8543836578b4e6eedd444972d4257b50e3fd949a` | `4effdcbfd5a6cf98c5972845be96946fe2048f23` |
| `input/fsh/valuesets.fsh` | `fce18bd7b106d87ce091c3ea980e9dfad88c438d58ae1c4d57a878a6661d3fd5` | `65412f5b421b7d1b490d1b1b9c030bfb33054622` |
| `input/images-source/gfi-004.plantuml` | `486ca6870b97ea65b0e46d3ab21da0ee31ac9d28e1a08a2939ea01552fe3b69e` | `e6b1ea80ab57ec94ea3a8c8eefbed14779c622cc` |
| `input/images-source/gfi-005.plantuml` | `b2d456b43e0d2f574ae83fd95fa240a10d85b53f3437892fe1cbff096c31839d` | `dd06e11d48277169edcb5985a283fc3c1616279a` |
| `input/images-source/localization-cardiologist-search.plantuml` | `8697bb9655a78ee9c6b0364ae98e4398674ad0bc3b464191cc6741d05bde323d` | `28b18d16c7bca99ceab1928691765bf51a99be3c` |
| `input/pagecontent/GFI-001.md` | `4fc8d23d28e46081420fc07b758e628223415c025470172391403d756fe75a45` | `527d4866f1bed18e54c4a1246114c396c49fdac3` |
| `input/pagecontent/GFI-002.md` | `337ae21eecbf8aadce755e0b22d475dbc675316e5b11d164e7868eaf962ad6ce` | `e666f809faebba683d998f85f2bb296e3393ddc3` |
| `input/pagecontent/GFI-003.md` | `572f20aa73b3e5d131b4d825a52d738cdeb4d68d5704e343709d2f48c0358098` | `9357920516371b1154f02bc8f65ee32c8fb57626` |
| `input/pagecontent/GFI-004.md` | `e4f03e4a65e0136fd7f837ee0c5cba693c4bc309b006a8f4d61a5f3994f27435` | `8f3986529f7185baa3b8161bf85d21c07a7ba0fa` |
| `input/pagecontent/GFI-005.md` | `f8faebc8e4638db55f13d6dda0b8a9e3d6031ff34f26a07939b64fb94b0e7deb` | `8a1aa573a7db6b01fff81879eb1dcfc62d241392` |
| `input/pagecontent/GFI-006.md` | `44235f7cd465551cd701855bee90621507f4edd1faa1399600c3af9e264df48a` | `4b861c0bd85180c85bdc85c02ba92d9c473b06c3` |
| `input/pagecontent/authentication.md` | `bc60f6adb5749e2589b233c49d710b757a720bdbc5af17a5737c44ebaa541f8a` | `09d30181f07f508cbb0b2b921cdec407139de977` |
| `input/pagecontent/authorization.md` | `51711138c1133698c3f4bd46e172ecf342353d70788003fcbbba94ad50d3c8c2` | `7a00835351f0ffabc3b5477e9c9edbbe3f2f69b7` |
| `input/pagecontent/care-services.md` | `92453985aa85d32e4fed8273e63fe9fd4ac26362077ded3987c68a1074214fa1` | `020382547e0f1fd1ccf9687c5bc120bb399e72b7` |
| `input/pagecontent/consent.md` | `411771e798049ead3e49bb6797ec80cea06516abac511b5ae8c1813cdf1ef399` | `6b5bcbe04bf6f72f41b180fd61281421084d935c` |
| `input/pagecontent/identification.md` | `87ab1299037c3110e4f467f966004c3a6a9e2a435ad0d9fb2bd02033aa2588a1` | `a2343e0a04a89900fd944239180d52f5198ed378` |
| `input/pagecontent/index.md` | `afeb6582b12b65d6f58efc82e0582986ef2f825ff5359b313a291d46dcb561c4` | `0e172f4a51c51e6002e65540dd9c6429272eca61` |
| `input/pagecontent/localization.md` | `8a70dc05bd48eea36a93b523eede2d1d6101ed13afc3045ac6f7a47b71947b17` | `34ccac5cb6a5afe528654125ac2420662de58136` |
| `sushi-config.yaml` | `faee15390b057138cfd6b559e0e04e26716291e85f306113b663292ba5528c64` | `ce0b01c550f63624626e42fa4bdf02154b3e81a8` |
