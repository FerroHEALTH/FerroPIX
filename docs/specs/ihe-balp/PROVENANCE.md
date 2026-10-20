<!-- This file describes vendored third-party material; the bytes beside it
     keep their upstream licence, not the licence of this repository. -->

# Provenance: the IHE BALP FHIR package

Vendored verbatim by `scripts/vendor/ihe-balp.sh`. Never edit a file here:
change the pin in docs/VERSIONS.md and re-run the script.

- Source: <https://packages.fhir.org/ihe.iti.balp/1.1.4>, the FHIR package registry's copy of the IG published at
  <https://profiles.ihe.net/ITI/BALP/1.1.4/>
- Pin: package `ihe.iti.balp` version `1.1.4`, tarball sha256 `be46dda3088ee9d486d7163458dc5a4d7192e8a1c587fd47e2beb70e4bcefa90`
- Fetched: 2026-10-20
- Upstream licence: Creative Commons Attribution 4.0 International
  (`CC-BY-4.0`, the `license` of the package manifest, listed under What is
  left out;
  <https://creativecommons.org/licenses/by/4.0/>). The package ships no licence
  file of its own. Attribution: IHE International, IT Infrastructure Technical
  Committee, *Basic Audit Log Patterns (BALP)* 1.1.4.
- FHIR version: 4.0.1
- Layout: the upstream paths inside the package, unchanged
- Files: 125 of the package's 126, listed below
- Tree digest (sha256 over the sorted per-file `sha256  path` listing,
  `PROVENANCE.md` excluded): `f7e73b45e097e9ddb135013454fcf74547fea01bb3dcee5216b5ca27137a4381`
- Read by: the audit records of `crates/ihe-iti` (feature `audit`), whose
  tests hold each record to the pattern its transaction's audit profile
  derives from and a search to the client-side example. The tree is the one
  FerroFED and FerroAUDIT vendor at the same pin.

## What is here

The whole package but its manifest: every audit pattern (the RESTful Query,
Read, Create, Update and Delete patterns and their Patient variants, the
OAuth and SAML token-use, consent and privacy disclosure patterns), the Audit
Creator, Audit Consumer and Audit Record Repository capability statements,
the code systems and value sets, the IG's examples, the OpenAPI and
Schematron renderings and the registry's index and validation output.

| File | sha256 |
|---|---|
| `package/.index.db` | `c3e52fa856a55db29980d60a2f15c9a0e7c27b94ecf87c6f7ba76555d6d77a9b` |
| `package/.index.json` | `684b7965c4f259d2909a49ae382e19d133442f699cb7fdabf706c96fdaf3f086` |
| `package/CapabilityStatement-IHE.BALP.ATNA.AuditRecordRepository.json` | `f553800b7cad86c39031908ea91e43f69ecffbafe86a499c7bc794edd62183bb` |
| `package/CapabilityStatement-IHE.BALP.AuditConsumer.json` | `9f2f06bb49fb48f4af634639c71f873641261859b64ecc8defafb0ca06b60e60` |
| `package/CapabilityStatement-IHE.BALP.AuditCreator.json` | `206261e7aab7ed84184b396060f2e23727c0609a77c8a5c17f9e9dfc00fbbd8e` |
| `package/CodeSystem-AuthZsubType.json` | `e631f2f62a0597e463dce12f4606bfb0e3424cd89eeb1480541894b45e4ed5bc` |
| `package/CodeSystem-BasicAuditEntityType.json` | `ade583a3ab87f9fc020b734f27ae8a8af814c55e40c8a6d378e9adad5cb8ed24` |
| `package/CodeSystem-OtherIdentifierTypes.json` | `4bdff24ca5ccfdaffbecd3e39c1da378c3ee3a69111b2f17f2d1658c36f7050b` |
| `package/CodeSystem-UserAgentTypes.json` | `56c13b791be76cd73e52f06acc7e1416b4fcff3efe5976e283baf294a744d2f0` |
| `package/ImplementationGuide-ihe.iti.balp.json` | `c025a67383dfb498479af43fb20b71c34b9af7183d44bc1cf7a30bfc6e0c1026` |
| `package/StructureDefinition-IHE.BasicAudit.AuthZconsent.json` | `1b38bbd59d9f885a1d0811f2f00df4f74f69abe43d248360719db8bd6b306380` |
| `package/StructureDefinition-IHE.BasicAudit.Create.json` | `5ffa75afb4fcefbb3d08fc0f8a7f801a465ce1f051800617d9a94de50cc57f11` |
| `package/StructureDefinition-IHE.BasicAudit.Delete.json` | `25843f721481d4abcfeb042cd509c342fcb957737700282af65c126a6d681f50` |
| `package/StructureDefinition-IHE.BasicAudit.OAUTHaccessTokenUse.Comprehensive.json` | `0034b4b1920d35556cdb3d3a1f339a27dde97d06fbdb26b2b676a04ad90f62ee` |
| `package/StructureDefinition-IHE.BasicAudit.OAUTHaccessTokenUse.Minimal.json` | `a74918e92452cfe08ddc0ab33c56724f7c0f5496b0ac3c7dded38412deb34c07` |
| `package/StructureDefinition-IHE.BasicAudit.OAUTHaccessTokenUse.Opaque.json` | `ec2ae508139b3c8ffc8f64036cb663971428955bcf9f16340e5be6ed189316f5` |
| `package/StructureDefinition-IHE.BasicAudit.PatientCreate.json` | `a4c1f4cae0b016d8fd913824f00021bdf190160834b065ad545ae8face0b437f` |
| `package/StructureDefinition-IHE.BasicAudit.PatientDelete.json` | `3bdcf3763440e36f1d69bb1ed2dfb148a5fa25ce451475fe60ef4e81f54ca30a` |
| `package/StructureDefinition-IHE.BasicAudit.PatientQuery.json` | `08fde79d720cc1a48118bcce4abbe9bb66f7cdd9cf81c8f51e7adca161978fd6` |
| `package/StructureDefinition-IHE.BasicAudit.PatientRead.json` | `70e9dafef3391dad9be416455d5a4cbb6ae33dd01b03b07045df46431d409aac` |
| `package/StructureDefinition-IHE.BasicAudit.PatientUpdate.json` | `9e9f77388012cabc8f3949a57049841d257b01f60da909a6e3351716dbbae103` |
| `package/StructureDefinition-IHE.BasicAudit.PrivacyDisclosure.Recipient.json` | `db50224b4cdecc8ce3e73b183a4d943ea10db82515e0162fcf0c31024a35706b` |
| `package/StructureDefinition-IHE.BasicAudit.PrivacyDisclosure.Source.json` | `f8082d82c5c4595f00f2668d5e080d2fa9ea45688dab86ca70ff5fc230701968` |
| `package/StructureDefinition-IHE.BasicAudit.Query.json` | `fd5c83e494e9af6da0782cbc0fde18897200bb19f07b21d81644aac0aa34e656` |
| `package/StructureDefinition-IHE.BasicAudit.Read.json` | `569e46ce96921d412d781841f1bd18b316b04294130916fffd99bf7991060e7e` |
| `package/StructureDefinition-IHE.BasicAudit.SAMLaccessTokenUse.Comprehensive.json` | `2a964522709baedcfd2ba38f176c695f96a93e677001cb41728af6eebebced50` |
| `package/StructureDefinition-IHE.BasicAudit.SAMLaccessTokenUse.Minimal.json` | `4895554d4eed101e38381945d413cc36b4292f347d570bc0cca3431d58c28f57` |
| `package/StructureDefinition-IHE.BasicAudit.Update.json` | `91b978fd2234eeca33a3414c81d293605bd088e3f656a2695a897e9092c1d925` |
| `package/StructureDefinition-IHE.IUA.71.json` | `256584bd261e115c1a83a0100b53bca15e818eec2e53705ad4af6ef988a7e26f` |
| `package/StructureDefinition-ihe-assuranceLevel.json` | `da0ffcb3bf23a9280d5a92b337678018cd72fcf72aa0385cde1f57a136942056` |
| `package/StructureDefinition-ihe-otherId.json` | `ef5c009b5afc2f07dfb9f4e4ecbcd8935a6b537d3e392bc1ddcca526f64b90c3` |
| `package/ValueSet-AllReadVS.json` | `45c272fdefa181b5273dcc79591534ab2adcf9dcc6795a808c0ca328bced1b7c` |
| `package/ValueSet-AllSearchVS.json` | `380e2113fd9e0e5c1403b1ad8b770443281bf6741a693c9814fcc52b589ced66` |
| `package/ValueSet-AllUpdateVS.json` | `7148d190f40125c735526d5dada235369f23d1185f3676678c641fae06c7bf55` |
| `package/ValueSet-AuthZsubTypeVS.json` | `9e53e4a48a86205ccc6d079ae1f6672e04576d2704a5160de26768414036541d` |
| `package/ValueSet-BasicAuditEntityTypesVS.json` | `15ea2882becba90e50f74e3dc01c83b009b701053774bac4b71a931075256d8d` |
| `package/ValueSet-DataSources.json` | `064a50a2ea7b92db7cca868864fd55c5652309623088ffc3ade72a37233d5674` |
| `package/ValueSet-OtherIdentifierTypesVS.json` | `1a93b75d15585ef378678048f7db01416fb7a85096a74ada7558d1bb047290bd` |
| `package/ValueSet-RestObjectRoles.json` | `254f7290a278fc205611146c230ff4e6e08063047c712eafd623822affd520a8` |
| `package/ValueSet-UserAgentTypesVS.json` | `610ab93a875876fbce63ae3717234dc4bd99a6b729474d5f2b49673c85559c5b` |
| `package/example/AuditEvent-ex-auditAuthZconsent-deny.json` | `66691ed8b9d17f37ef869982a3a7aea7a1dd11134bce3a66c822e9dcd8f6198a` |
| `package/example/AuditEvent-ex-auditAuthZconsent.json` | `3ca96e5ef34be78b9b8289068d5b2d63bc29f1f8befa24bbd0a29daf6d599186` |
| `package/example/AuditEvent-ex-auditBasicCreate1.json` | `aacbea9f017c38458d1aa9200ab6366f39371b14b078173320350132da189085` |
| `package/example/AuditEvent-ex-auditBasicCreate2.json` | `a32cca1709fc1488a14a241c5e184ac2a1a30f19c14ce986bb6a424496213f0c` |
| `package/example/AuditEvent-ex-auditBasicCreateClient.json` | `b063b905ebd558b9698f19f44be74d16301e0a07f77374d4b33fa3724bbd9bfd` |
| `package/example/AuditEvent-ex-auditBasicCreateNoPatient.json` | `15c3cc3f38be83f27eb6af2b493e98679cb30653ebfb3ae3574d2eeb0884a3db` |
| `package/example/AuditEvent-ex-auditBasicCreateNoUser.json` | `250c795f18fff95dbdc9ab79721b176d18f372bbca509f6502abf62a2ce8d8dc` |
| `package/example/AuditEvent-ex-auditBasicCreateNoUserJob.json` | `299041d280a62fd5d0eac7d58cb46fa30de73150e4a05eadc3898fb46d102f4a` |
| `package/example/AuditEvent-ex-auditBasicCreateNoUserReport.json` | `ac3540043417421b6cff98343d2f8a3dfed8c9cfe91421860f47d38d9ec72c56` |
| `package/example/AuditEvent-ex-auditBasicCreateServer.json` | `db112adf0439cb0535294d84b8e419c1edc4b12974bd23a3fdca563e700d8374` |
| `package/example/AuditEvent-ex-auditBasicDelete2.json` | `2560c4037693a77ce2caa05ac320dc2d53ef1f4beef5a3428d12663683e65b27` |
| `package/example/AuditEvent-ex-auditBasicDeleteClient.json` | `c52f558d0b9f331bf465b68bf62eb773359caf5e533750a30b2ac52f0b74ba24` |
| `package/example/AuditEvent-ex-auditBasicDeleteInformant.json` | `3e760e5e30b1cbbf7dbecff37c3d9f9b2a773326850e72ac26c82c98731472db` |
| `package/example/AuditEvent-ex-auditBasicDeleteNoPatient.json` | `6a397ea9436d973343f5937641f19b3be4218aee975c78d0ee3a4021a6df77e2` |
| `package/example/AuditEvent-ex-auditBasicDeleteNoUser.json` | `9989e5024fcbd3ea4f99ad0035546079258cdd775bade52b32cc2b35797aa87a` |
| `package/example/AuditEvent-ex-auditBasicDeleteNoUserJob.json` | `2633398f57b16870e18ed341b916cc44229316eb451e6a715de50bee9888ab18` |
| `package/example/AuditEvent-ex-auditBasicDeleteNoUserReport.json` | `a04fee20833c1139cf62922141f7fa563ff08c97c1a80df6bef28fb6ae022110` |
| `package/example/AuditEvent-ex-auditBasicDeleteServer.json` | `72457c3bf51ce7d67d6bf740b5ee2960f2c349e9dad911ccbf9c7d2fde97f27a` |
| `package/example/AuditEvent-ex-auditBasicPatch.json` | `ff2e6af55f44774e940bd9fc4e23b60194854d074ef897f431177e552fbbb215` |
| `package/example/AuditEvent-ex-auditBasicQueryGetClient.json` | `c5d73df6074bff3247b1a0215642d2934694c546d45a9a2865f08ca953c31f9b` |
| `package/example/AuditEvent-ex-auditBasicQueryGetNoPatient.json` | `12d916eb6af6c112ebbf03151de7a7d426dbf8f10c7dec318b3a55ef0da6d800` |
| `package/example/AuditEvent-ex-auditBasicQueryGetServer.json` | `f590abccfe2bad21d129e19d318c764af9c213693a96c14075d25fe5d74c2977` |
| `package/example/AuditEvent-ex-auditBasicQueryPost.json` | `5b7459afd452017bcd3d133184d748653e5056dd27ccfbdfcfceb1ceca257721` |
| `package/example/AuditEvent-ex-auditBasicReadClient.json` | `989f5d845f456eb6de455ec61ab1adbfcdc8a5e2e19e99e7e63cdc57f09b1620` |
| `package/example/AuditEvent-ex-auditBasicReadNoPatient.json` | `5c59f852e0826b329fe2d1413360f38650d62ac9998621045813d1b3a935508b` |
| `package/example/AuditEvent-ex-auditBasicReadNoUser.json` | `d55302897623f527a7677fe5ccad1a7ac6b47a0dd58e7cb07fda4965f65a9b14` |
| `package/example/AuditEvent-ex-auditBasicReadOClient.json` | `aacb2e125523130bab43933e5c185d143290a2ba321760da12af33e6b1fc1ff3` |
| `package/example/AuditEvent-ex-auditBasicReadOServer.json` | `1af61b1619bdacab962c4203d741bb2e39ebfc2f55fe24175139ca10da252631` |
| `package/example/AuditEvent-ex-auditBasicReadOServerMin.json` | `6cdc732018af01f81fbdd1cf679bd0a81482714b6c3eb24af74f0e9a03b37134` |
| `package/example/AuditEvent-ex-auditBasicReadServer.json` | `9342a1f7d8b1d4a1549ef94a814030ffce5b88c61ace5975a36b5a36bb283f3a` |
| `package/example/AuditEvent-ex-auditBasicUpdate1.json` | `59fcbf7861e583d56c055e7a946eef8bd7025438314412f73bde44456f1ab6f4` |
| `package/example/AuditEvent-ex-auditBasicUpdate2.json` | `2d950b1e710fa6a7d8830d44db36f17347e9aca11b96b911d8daf747bf9a872d` |
| `package/example/AuditEvent-ex-auditBasicUpdateNoPatient.json` | `80a99ea78ff78c0865be8539efffd7d070ce82a71195f70f282ed449c4dc8535` |
| `package/example/AuditEvent-ex-auditBasicUpdateNoUser.json` | `de35f653077486f471dc7689d04dbf7f190c382f546e7d2b35b488787748af15` |
| `package/example/AuditEvent-ex-auditBasicUpdateNoUserJob.json` | `8af2bd73e705fc4d17d8b64d46703c023f3c99c634f5b8db96a9a8f0ffa8672e` |
| `package/example/AuditEvent-ex-auditBasicUpdateNoUserReport.json` | `6d50c67133bd8db803cedc99eae871bf3706d1791dd1c42d213bf420a1cfe4a6` |
| `package/example/AuditEvent-ex-auditPoke-SAML-Comp.json` | `021fa95fd1dca316df88633ad0cf9e1f0d9e30a51ceff23879e737cef58d3b21` |
| `package/example/AuditEvent-ex-auditPoke-SAML-Min.json` | `eeb7b325cebcb02d59bd0b98a0a5b1faa09212f5280e058a4f290b1793bb1b12` |
| `package/example/AuditEvent-ex-auditPoke-SAML-Min2.json` | `338483237460c6140f3637db1cf0a06c093feca88ffc1c0d1017570a9829a443` |
| `package/example/AuditEvent-ex-auditPoke-SAML-QDI-Comp.json` | `8b78739e454e7ddf3f051d0f3f7921db433e8e4255d5de767385a75d2cb2db52` |
| `package/example/AuditEvent-ex-auditPoke-SAML-QDI-Min.json` | `b0d0cf14ef189f75c574580f6093f134c4a5b101a2cab268e607830fc8b0aaec` |
| `package/example/AuditEvent-ex-auditPrivacyDisclosure-measurereport.json` | `f6372a41f28184da51659534edb1bc687afffd444d3ad90cead6fc72d53f42bf` |
| `package/example/AuditEvent-ex-auditPrivacyDisclosure-recipient-minCodes.json` | `083f81a688954685aa9d65def637401bde0379d8da7ddd8dc63ce3ba48755e14` |
| `package/example/AuditEvent-ex-auditPrivacyDisclosure-recipient.json` | `1962ac4a3b0b9c5e4020de9aa5038e4332808a8c08ffb48bd767155361390551` |
| `package/example/AuditEvent-ex-auditPrivacyDisclosure-source.json` | `4eef9f857436ee035e0e7dbba825aa44d1f83cbf6b33d3f2cd74af6a57998159` |
| `package/example/AuditEvent-ex-auditPrivacyDisclosure-source2.json` | `85e651b9583012994b4663300124482834b12dbf62bef95207f880210e1ae7e3` |
| `package/example/Binary-ex-b-binary.json` | `1803d326940f3e1b656ebe67c97af71317173db4a7b7b5778d48abcbc0f05f15` |
| `package/example/Consent-ex-consent.json` | `8a25809baec6483dd04bd68e37902e7ab75c343531c2be797ef611f8740ef0b0` |
| `package/example/Device-ex-authz.json` | `9f6b4d0c57a4a975b4d852d7895ccf8cd476caa107c6271a2ecb1cc2688803e2` |
| `package/example/Device-ex-device.json` | `be7819d7a1f1829bc42ed34d832f10361e71875c7d111d3921aad12d2d8298fb` |
| `package/example/DocumentReference-Dr-SAML-QDI.json` | `e7ee4bbf2871e06caf85cd0192f9d2b8bcaee086e3e66e41fb512246e22be426` |
| `package/example/DocumentReference-ex-documentreference.json` | `1ffd738bd7d13a70f66721722585f9c9b213f4cc907376bb674db7c68b1ee600` |
| `package/example/DocumentReference-ex-documentreference2.json` | `01b5f777915de3e0b229a6c7e665a2abe6fbb9b8c43c722483dddf9e8c771141` |
| `package/example/List-ex-list.json` | `d3ce3a119c363eee9354d4fc774f0eaae4fce9b0d27b24a0143dc9c4f8af7a2c` |
| `package/example/MeasureReport-ex-measurereport.json` | `a76d97771d728d6d9b26f0d9b65c4572a02a0f37b13fbee9867856f5a8075d55` |
| `package/example/Organization-ex-organization.json` | `e70d0b6fc5ea345bd3ed7d9ac22d24119a1eb1919e2063e9894fe7a819365c72` |
| `package/example/Patient-ex-patient.json` | `114b5f86edd1f177412f8833ea2d48619c9482b0d7de9109de897a12d729ca45` |
| `package/example/Practitioner-ex-practitioner.json` | `f3e5048932ad87f2625e2a678590c747ca6039a611ad5183e2262c24c77ddf05` |
| `package/openapi/IHE.BALP.ATNA.AuditRecordRepository.openapi.json` | `dea0718c8bada4ddaf6946553572a9e49ee98c5b3b67e163287ed52fb4dff379` |
| `package/openapi/IHE.BALP.AuditConsumer.openapi.json` | `9d526ce6594b63f9604106dbdee0eaddc188f594becaa932114bee003e1c7ae4` |
| `package/openapi/IHE.BALP.AuditCreator.openapi.json` | `d078821d53731edb54dc8bcb183066457d185b28f0bd8eeab7a9e60db7d76839` |
| `package/other/spec.internals` | `989597b598186695d416df6b8ecd1180622af5d7fbf7360221111f75ea7fc34f` |
| `package/other/validation-oo.json` | `18782e0cdade3c2c5918eb0343ddff8b78b09688cf5eb0e07fdc04eb131f5add` |
| `package/other/validation-summary.json` | `03b72e467c7dba516a9295dbb412b6e62a58698ddfd96fd04c55b93715b6cf40` |
| `package/xml/StructureDefinition-IHE.BasicAudit.AuthZconsent.sch` | `852a6bdf825111210d00d4371cd5d9b584a3db9d2430cc1f114eb23ef6629021` |
| `package/xml/StructureDefinition-IHE.BasicAudit.Create.sch` | `9fb985a662f0e956ac8051ff9cbae9bb282367c9a37743ae2cd5489dd0e0bd9e` |
| `package/xml/StructureDefinition-IHE.BasicAudit.Delete.sch` | `9fb985a662f0e956ac8051ff9cbae9bb282367c9a37743ae2cd5489dd0e0bd9e` |
| `package/xml/StructureDefinition-IHE.BasicAudit.OAUTHaccessTokenUse.Comprehensive.sch` | `71b4f33b86498b1b7b2290b8efe144e99799654aff4369798ec41504eed9e01f` |
| `package/xml/StructureDefinition-IHE.BasicAudit.OAUTHaccessTokenUse.Minimal.sch` | `8944031ffda66ca801ec12b6175f27ea8f7bc54a1de8cab1a7d85aeec21fd9b6` |
| `package/xml/StructureDefinition-IHE.BasicAudit.OAUTHaccessTokenUse.Opaque.sch` | `92b7906bc23566acea0fa16e2227f7b5c0e41dc711d5c562abe58084f4dcf930` |
| `package/xml/StructureDefinition-IHE.BasicAudit.PatientCreate.sch` | `cf4524251596faddca853acdacb599bff5079db6d48e1642e26ab3e905430f8e` |
| `package/xml/StructureDefinition-IHE.BasicAudit.PatientDelete.sch` | `6679fbeaa1b67b79515d6e89291e91f0e06637b2b54b8b9c674616bd06491861` |
| `package/xml/StructureDefinition-IHE.BasicAudit.PatientQuery.sch` | `aee3e6cfc1a743919d4d529eb3a4531bc424a8084948f6cfa15582870edb618c` |
| `package/xml/StructureDefinition-IHE.BasicAudit.PatientRead.sch` | `3558be3e7b7cf6cec23a03297823e83ef4f504f6e1a61fad63a549f6d5b38785` |
| `package/xml/StructureDefinition-IHE.BasicAudit.PatientUpdate.sch` | `fc5073c994a247d6ea665e9e63417c7b1ab22be8cef230e54cb193d5461e96e5` |
| `package/xml/StructureDefinition-IHE.BasicAudit.PrivacyDisclosure.Recipient.sch` | `a7315f7e17c0b88fbb982cf6695cce8c81443e2d1562fd127569f48c1d5b7586` |
| `package/xml/StructureDefinition-IHE.BasicAudit.PrivacyDisclosure.Source.sch` | `a7315f7e17c0b88fbb982cf6695cce8c81443e2d1562fd127569f48c1d5b7586` |
| `package/xml/StructureDefinition-IHE.BasicAudit.Query.sch` | `363a3871d09d5757cf56cbe5e503fad92f9d92683bad74c7e5154aeed8b9c931` |
| `package/xml/StructureDefinition-IHE.BasicAudit.Read.sch` | `fc83897bca0f77234b183b7e5643630b2c57da8f006e5fb2d40801ca1aef77a5` |
| `package/xml/StructureDefinition-IHE.BasicAudit.SAMLaccessTokenUse.Comprehensive.sch` | `e78bc1b803600a103a069fd2dcbc159a885c232edc43a707106cb3138bc82375` |
| `package/xml/StructureDefinition-IHE.BasicAudit.SAMLaccessTokenUse.Minimal.sch` | `0a3fe10eb028e76b65df0d784db38c6408aef8b9909e2b1b13d05131ba3896c3` |
| `package/xml/StructureDefinition-IHE.BasicAudit.Update.sch` | `9fb985a662f0e956ac8051ff9cbae9bb282367c9a37743ae2cd5489dd0e0bd9e` |
| `package/xml/StructureDefinition-IHE.IUA.71.sch` | `abcb925564bb986d2e8b8f7097e402b069e854ff3b524cc3e2f82064d54fbcb3` |
| `package/xml/StructureDefinition-ihe-assuranceLevel.sch` | `2c83a9893e1649c9b50882084dd8932f3eb20909c087cca12359e8f9824ebfc9` |
| `package/xml/StructureDefinition-ihe-otherId.sch` | `603d41a563fab05927d9fbf4e498af080f917d5ac1d09c5d9e62f22acd09daac` |

## What is left out

The package manifest, `package.json`. The script reads its name, version
and licence from the tarball and checks them against the pin. A vendored copy
would make this repository's dependency graph claim an npm package that
depends on `hl7.fhir.r4.core`, a FHIR registry package whose name the GitHub
advisory database flags as a malicious npm package; nothing here installs
either.

| File | sha256 |
|---|---|
| `package/package.json` | `82c2985c0bd83becbcd82a7212f42337b48deb2eccf66ef87ee07eebef7b4a58` |
