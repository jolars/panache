# Changelog

## [2.43.0](https://github.com/jolars/panache/compare/v2.42.0...v2.43.0) (2026-05-06)

### Features
- **linter:** add rule for stray fenced div markers ([`3b6ebe9`](https://github.com/jolars/panache/commit/3b6ebe9a12f99adefeba1452eec681a54fc20e88)), closes [#255](https://github.com/jolars/panache/issues/255)
- **linter:** flag near-misses too in html entity rule ([`ce47752`](https://github.com/jolars/panache/commit/ce477525b03f52927cd7c0ef5b89747df6069984)), closes [#251](https://github.com/jolars/panache/issues/251)
- **editors:** bundle binaries in VSIX extension ([`8a98d01`](https://github.com/jolars/panache/commit/8a98d014aaa6d50779bf944656b0350cd445a2d5))

### Bug Fixes
- enable `autolinks` for GFM ([`aeda13c`](https://github.com/jolars/panache/commit/aeda13cdc71a002bf0326cab9c1354abec321b2a)), closes [#258](https://github.com/jolars/panache/issues/258)

### Dependencies
- updated crates/panache-parser to v0.7.1
## [2.42.1](https://github.com/jolars/panache/compare/v2.42.0...v2.42.1) (2026-05-05)

### Bug Fixes
- **editors:** remove links in docs to pass vs code check ([`f9240fb`](https://github.com/jolars/panache/commit/f9240fb2b478ad136359625ed898c587bc82eaa0))
## [2.42.0](https://github.com/jolars/panache/compare/v2.41.1...v2.42.0) (2026-05-05)

### Features
- **linter:** add linting rule for bad HTML entities ([`93aa280`](https://github.com/jolars/panache/commit/93aa2804dcd6d874d2c02b149ecead83233d9bc0)), closes [#251](https://github.com/jolars/panache/issues/251)
- wire new reference impl into salsa and CST ([`3ba22c1`](https://github.com/jolars/panache/commit/3ba22c1700591cd6d1c173d74416c97987a33fa0))
- add `parse_with_refdefs` and `UNRESOLVED_REFERENCE` ([`e6c17fb`](https://github.com/jolars/panache/commit/e6c17fb6f2903c74bbe547b19200abcb381dcc4d))
- **cli:** add `panache parse --to pandoc-ast` output mode ([`f0f9ace`](https://github.com/jolars/panache/commit/f0f9acea550e60356a1a35a0f3fa82d4700642c9))
- **parser:** expose pandoc-native projector as public API ([`5b79b92`](https://github.com/jolars/panache/commit/5b79b92647fe889fcd1179e1145902bb4588f22e))
- **editors:** add `executableStrategy`, deprecate old options ([`884a3c4`](https://github.com/jolars/panache/commit/884a3c4d851904b01f5644ad1ca350f013636247))
- **editors:** add log-level setting ([`a6cf4c7`](https://github.com/jolars/panache/commit/a6cf4c711074073ce7e71046806edebdbad058fd))
- **editors:** add restart server command ([`ab5cfb0`](https://github.com/jolars/panache/commit/ab5cfb043bf56586952f726b4344b71eb8d7604a))
- **editors:** register as default formatter for rmd & qmd ([`889d640`](https://github.com/jolars/panache/commit/889d640d165a5b7eb98caf5424b2573a486e1720))

### Bug Fixes
- **parser:** degrade unresolved bracket if inner emph leaks ([`e1c291b`](https://github.com/jolars/panache/commit/e1c291b0b2f478324e91e90e4895333d099c89e9)), closes [#250](https://github.com/jolars/panache/issues/250)
- handle ambiguous markers and indented code block ([`8d3db6d`](https://github.com/jolars/panache/commit/8d3db6d5937137ae825523f0f8141edcdd200fa4))
- **parser:** allow drift tolerance for list parsing ([`1836a7b`](https://github.com/jolars/panache/commit/1836a7b748c127ffe794a137df91940f30567382)), closes [#246](https://github.com/jolars/panache/issues/246)
- **formatter:** handle nexted list with same line marker ([`8d0653a`](https://github.com/jolars/panache/commit/8d0653a69c1dda3b3a0f07a813c7a44e4efe3766)), closes [#247](https://github.com/jolars/panache/issues/247)
- **parser:** handle tilde-fences dispatch correctly ([`519abd1`](https://github.com/jolars/panache/commit/519abd1c12dff37331e9aad3d2baefe4b7701fb9)), closes [#248](https://github.com/jolars/panache/issues/248)
- **cli:** link to correct reference page for rules ([`c807d70`](https://github.com/jolars/panache/commit/c807d70f8a2acc08cd04ca5a9eb921f071458b4b)), closes [#245](https://github.com/jolars/panache/issues/245)
- recursive into linst/blockquote/list ([`175d78e`](https://github.com/jolars/panache/commit/175d78e6ce5287578fe7c7ee5c3c079e674f2663))
- handle lazy-continuation for blockquote + list ([`4a490ff`](https://github.com/jolars/panache/commit/4a490ff25df2d09b8405aef3756a51f85b925e39))
- allow continuation list without blank line in definition ([`daed645`](https://github.com/jolars/panache/commit/daed645a295715108ad25a4c36f1d18bad00a57f))
- peek-ahead in blankline in blockquote ([`74adea6`](https://github.com/jolars/panache/commit/74adea62a08920d021c514ef4c58e92fca0a93f8))
- handle pandoc-commonmark divergence on html comments ([`ca301f9`](https://github.com/jolars/panache/commit/ca301f99a4dc74d7d40ad087d59f97928cff5fc4))
- handle same-line block quote marker ([`3c6c3dd`](https://github.com/jolars/panache/commit/3c6c3dd7739ed592d3f6e6c7305a9d616a953fb2))
- **parser:** handle direct list-in-lis correctly ([`5c6a4ae`](https://github.com/jolars/panache/commit/5c6a4ae6ac476232ef6040df586610cfc13f44ef))
- correctly handle definition inside footnote ([`3a30b05`](https://github.com/jolars/panache/commit/3a30b0588acb6a023389fc04604b0ff01d3d6ce4))
- correctly parse and format definition with bare list ([`72c9a2b`](https://github.com/jolars/panache/commit/72c9a2ba960eaf2431e2b81f9fc2f3ace5f1920b))
- parse and format headings inside lists ([`d7e714e`](https://github.com/jolars/panache/commit/d7e714ebab500156d6e5a3b5887173f9ea1e6402))
- **parser:** fix early-bail to not fire incor for strikeout ([`f486309`](https://github.com/jolars/panache/commit/f486309b4c32699be3beef9f181936f809ac3b10))
- **parser:** require two spaces after roman marker ([`8d7255f`](https://github.com/jolars/panache/commit/8d7255f1bd5476e7e8c0af50a932f1f7593afde4))
- **parser:** allow unindented block to follow atx heading ([`bf84aa1`](https://github.com/jolars/panache/commit/bf84aa1667655456ab45716fe0a9aa3110854d9e))
- **parser:** fix byte-order breakage in tilde-fenced code ([`18ca6c2`](https://github.com/jolars/panache/commit/18ca6c2bec5e46ee241df774e772f2e37105ed5a)), closes [#249](https://github.com/jolars/panache/issues/249)
- **editors:** update package lock file and get rid off uuid ([`b02ef2c`](https://github.com/jolars/panache/commit/b02ef2cf9f946500a07fdb421a0262062a47dcc5))

### Performance Improvements
- **lsp:** share salsa-cached refdef set across LSP keystroke parses ([`e1e2927`](https://github.com/jolars/panache/commit/e1e29278f41082d8d382fc2ba7470a8f7a45db47))

### Dependencies
- updated crates/panache-formatter to v0.4.2
- updated crates/panache-parser to v0.7.0
## [2.41.1](https://github.com/jolars/panache/compare/v2.41.0...v2.41.1) (2026-05-01)

### Bug Fixes
- **formatter:** extend block-token list ([`d087729`](https://github.com/jolars/panache/commit/d08772922a3b983612fb29e3f0a1ed90510a66ff)), closes [#238](https://github.com/jolars/panache/issues/238)
- **parser:** suppress nested links in Pandoc link text ([`b8e1c9a`](https://github.com/jolars/panache/commit/b8e1c9ad31bed5c6180c08c4de57faf81450e05e)), bugs [#1](https://github.com/jolars/panache/issues/1) and [#2](https://github.com/jolars/panache/issues/2)
- **parser:** handle Pandoc emphasis on the IR path ([`afa0ef5`](https://github.com/jolars/panache/commit/afa0ef5e3a202dae86ff1b4a282618b35a34f413))
- **parser:** finish milestone - full commonmark compliance ([`33a88e8`](https://github.com/jolars/panache/commit/33a88e89ac573872a0a7ec26ea9e9e5b0ace5d64))
- **parser:** implement IR algorithm ([`bb91c85`](https://github.com/jolars/panache/commit/bb91c850dbf790895ab01e233aacde1debd544a5))
- **formatter,parser:** handle setext in list ([`86494b5`](https://github.com/jolars/panache/commit/86494b57765e2c2a8eae7b1183018774bd99fecc))
- **parser:** fix emphasis parsing for cmark ([`de1b406`](https://github.com/jolars/panache/commit/de1b406bca16c390452cc9c3605a31edcbab28de))
- **parser:** handle empty maker followed by indented content ([`6a9b188`](https://github.com/jolars/panache/commit/6a9b188fc8ac53bb2130dc9cd3394919aaeeb839))
- **parser:** open inline blockquote for commonmark ([`a2ad903`](https://github.com/jolars/panache/commit/a2ad903f478552dbef53c374b441ebe802ab2eec))
- **parser:** handle rule of 5 cols for commonmark ([`dcb36e6`](https://github.com/jolars/panache/commit/dcb36e63801223549e038a39c009a0d2ecc9fcfb))
- **parser:** honor source-column tab stops ([`15ebe05`](https://github.com/jolars/panache/commit/15ebe058943fdb053d5a3eb1c7cd918d34fcb329))
- **parser:** make fenced code openers interrupt paragraphs ([`f9a3b50`](https://github.com/jolars/panache/commit/f9a3b5021900151d6d56998b2f68a9ef8d15c60a))
- **parser:** handle two tab cases in commonmark tests ([`3bf2140`](https://github.com/jolars/panache/commit/3bf2140dd4015e67abe7c6c0f7ba72484dd9d8e4))
- **parser:** don't allow links to contain links in cmark ([`52eb5f2`](https://github.com/jolars/panache/commit/52eb5f248ab8e817a3364eba62b2c06a7c9184b2))
- **parser:** handle last HTML block edge case ([`3a13337`](https://github.com/jolars/panache/commit/3a13337455a7c950d5692bd81297f2014ca4862a))
- **parser:** handle dialect-specific list item closing ([`c61f93b`](https://github.com/jolars/panache/commit/c61f93bddd5faa256edf412b9350a739d6b9fd6c))
- **parser:** handle last refdef dialect mismatch ([`245543b`](https://github.com/jolars/panache/commit/245543bbbb8ca87496e8aca7d881486731526b64))
- **parser:** handle last block quote discrepancy in cmark ([`0fce82a`](https://github.com/jolars/panache/commit/0fce82a7d7c8273d8d401ca4ef3920da31a70760))
- **parser:** correctly handle non-uniform list indents ([`f7750dd`](https://github.com/jolars/panache/commit/f7750dde57c23d8b9e531e370870a2a6b33b4540))
- **parser:** handle continuation in block quote better ([`2f209e5`](https://github.com/jolars/panache/commit/2f209e51b1d73e7abbad2b09b5bd435120f9f653))
- **parser:** implement better link scanning ([`eaca3a1`](https://github.com/jolars/panache/commit/eaca3a1323ac81b888a25b8572e77e0dbb2f4d69))
- **parser:** don't skip code spans in closer scan ([`687e908`](https://github.com/jolars/panache/commit/687e9087fd481679ac0161200a2cfacc91fdad94))
- **parser:** allow partial emphasis matching for commonmark ([`e172b52`](https://github.com/jolars/panache/commit/e172b52b6772df3a43d296f9c0e3ff8884f54e98))
- **parser:** recurse inte same-line nested lists markers ([`ac05e88`](https://github.com/jolars/panache/commit/ac05e88d7addd1e8eef3caa6bf2bf36568e67b66))
- **parser:** handle emphasis edge case ([`1b13a73`](https://github.com/jolars/panache/commit/1b13a73a970af4c2e8ac8d0a365bf5ec40b017ac))
- **parser:** improve cmark emphasis parsing ([`95b2811`](https://github.com/jolars/panache/commit/95b281120d7beafb3cfda494d4b7ec617784c717))
- **parser:** handle edge-cases for cmark emphasis ([`be57d7d`](https://github.com/jolars/panache/commit/be57d7d95343dec133c3b3955a752f407b35ad8c))
- maintain list markers for commonmark ([`084fc87`](https://github.com/jolars/panache/commit/084fc870805fa1fe8b4b36fcfe0c4b06f2a23a43))
- **parser:** relax indented-code opener ([`c0dcfb7`](https://github.com/jolars/panache/commit/c0dcfb7472c301afe2044dd461ca54966f78af06))
- **parser:** support multiline setext headings ([`4b4e1a3`](https://github.com/jolars/panache/commit/4b4e1a3b90e78c8ca0b981051d68dbf33805faad))
- **parser:** handle parser losslessnes from emphasis ([`0104a7c`](https://github.com/jolars/panache/commit/0104a7c390b60639de6ac823b03811004a2d3dce))
- **parser:** don't let `]` terminate a link inside code span ([`18e028d`](https://github.com/jolars/panache/commit/18e028dd2d28af7561f3b3bff67a265a2811323f))
- **parser:** fix parenthesis tracking ([`d37ba7d`](https://github.com/jolars/panache/commit/d37ba7d9c2e24918c049ed3014cb854d255c269f))
- **parser:** properly handle multilevel ref def ([`50f28f4`](https://github.com/jolars/panache/commit/50f28f47475a739732d2133667fc7e1b01990d9e))

### Performance Improvements
- **parser:** add IR-driven dispatch for Pandoc links/images ([`1e4227e`](https://github.com/jolars/panache/commit/1e4227e94e1c110f99a4e5185f3b13cdc58825d5))
- **parser:** add IR-driven dispatch for [text]{attrs} ([`cf50ec5`](https://github.com/jolars/panache/commit/cf50ec5c7d5572bad8a6b5989c34e7b0c593a12a))
- **parser:** add IR-driven dispatch for citations ([`9e826db`](https://github.com/jolars/panache/commit/9e826db3c488fecb821f42a22410a34297690b18))
- **parser:** add IR-driven dispatch for [^id] footnote refs ([`614221e`](https://github.com/jolars/panache/commit/614221e5b9d0d2819b50abdd6d499fd87509c8c2))
- **parser:** add IR-driven dispatch for ^[note] and <span> ([`1b9e618`](https://github.com/jolars/panache/commit/1b9e61876896c36964dba36ffdc60bcf489c7309))
- **parser:** early-exit + scratch reuse ([`c2c0387`](https://github.com/jolars/panache/commit/c2c038771c2ff70cc3663185b8e64d862553cbdd))
- **parser:** add leading-byte gate ([`c851afe`](https://github.com/jolars/panache/commit/c851afe1866a9ee50214b10445ca2b03c11b5b91))
- **parser:** add byte-level blank-line check ([`7530c25`](https://github.com/jolars/panache/commit/7530c25d2843493ca1553ba8656ecba24a4032c8))
- **parser:** add byte-level link-suffix whitespace skips ([`89b31e4`](https://github.com/jolars/panache/commit/89b31e461d209f790435c13837aba3b30957aeda))
- **parser:** skip exclusion-mask pass when no brackets ([`92ec5db`](https://github.com/jolars/panache/commit/92ec5dbba1f579a1b128c4c2d7517e1f2841bd22))
- **parser:** byte-level is_blank_line on blank-check paths ([`fab385e`](https://github.com/jolars/panache/commit/fab385e81f0b9fa00c829ecd04a1fc338526c37b))
- **parser:** leading-byte gate in collect_refdef_labels ([`7058785`](https://github.com/jolars/panache/commit/7058785352d5a186320dee834c46e088318188f6))
- **parser:** zero-alloc Roman numeral check ([`ff4d3eb`](https://github.com/jolars/panache/commit/ff4d3ebd7362644e379c27e7569f4abd44538879))
- **parser:** leading-byte gates on hot block parsers ([`57f9f69`](https://github.com/jolars/panache/commit/57f9f6923e07d22b90b869389aa5bc466c53116f))
- **parser:** memchr-based code-span scan + zero-alloc ([`490d593`](https://github.com/jolars/panache/commit/490d59375234454c426078df2c352f6c583a0f57))
- **parser:** byte-level trim helpers on hot per-line paths ([`a63a02a`](https://github.com/jolars/panache/commit/a63a02a6b4257ef9b37abcd1af68209d6fd9842b))
- improve performance on the IR path ([`44d6d5b`](https://github.com/jolars/panache/commit/44d6d5b3cde148c76cb51210d1b329ec4977d013))

### Dependencies
- updated crates/panache-formatter to v0.4.1
- updated crates/panache-parser to v0.6.1
## [2.41.0](https://github.com/jolars/panache/compare/v2.40.0...v2.41.0) (2026-04-29)

### Features
- **parser:** handle inline HTML ([`5fb7272`](https://github.com/jolars/panache/commit/5fb727257c0b2d6385b22e29a64f2bde1d0196f4))
- add `Dialect` to untangle CommonMark from Pandoc ([`a1cb7df`](https://github.com/jolars/panache/commit/a1cb7df9ca8461f45db2b7f4efb50e57e8febce3))
- add npm package ([`a8e86b2`](https://github.com/jolars/panache/commit/a8e86b207af62119065a0de0274a653a8800381c))

### Bug Fixes
- **parser:** respect escapes inside reference definitions ([`2ec4025`](https://github.com/jolars/panache/commit/2ec402586d143d076041bcb5ebd44fd4fea0c95e))
- **parser:** allow fancy lists in core cmark, improve logic ([`191f636`](https://github.com/jolars/panache/commit/191f63671c2f3502be516f1f5f8ee506d8265d61))
- **parser:** don't allow ref defs to break paragraphs ([`b05e3f3`](https://github.com/jolars/panache/commit/b05e3f3afd58527992c9b4c6df4c91d60b6c821c))
- **parser:** allow breaks in reference links ([`7da4875`](https://github.com/jolars/panache/commit/7da487518a0ee90736e68247c887ce25a9d4484f))
- **parser:** for cmark, cap digits for lists at 1-9 ([`39ba64b`](https://github.com/jolars/panache/commit/39ba64b9f6c7aab566150f58fe49641b79f7f740))
- **parser:** correctly handle empty list items ([`1143607`](https://github.com/jolars/panache/commit/11436073c2aa73badc411c3366195f65ad52c7a0))
- **parser:** properly handle fenced code inside list items ([`6b6ccdd`](https://github.com/jolars/panache/commit/6b6ccddcdc07940bdec2ee2ce4f3bda3e514a165))
- **parser:** make blanklines inside list item a loose list ([`23d7a90`](https://github.com/jolars/panache/commit/23d7a9042518bdbf51f0a368309fd91eb500d596))
- **parser:** handle ruler as only list item ([`a1004e6`](https://github.com/jolars/panache/commit/a1004e66c6a4e6404ded859a997405e24d85eb3e))
- **parser:** handle thematic breaks and setext headings ([`a02c3d5`](https://github.com/jolars/panache/commit/a02c3d50eaa038fc6c4ab0f5f20f28db3e28b8ef))
- **parser:** handle autolinks and blockquotes for cmark ([`b1cedd4`](https://github.com/jolars/panache/commit/b1cedd4f586ea53b7174a039d37f2160c1dcdfab))
- **parser:** handle HTML blocks for pandoc/commonmark ([`227648e`](https://github.com/jolars/panache/commit/227648e07760c65282372dab159ca50bb5e32f09))
- **parser:** handle pandoc/cmark difference in fenced code ([`b370edd`](https://github.com/jolars/panache/commit/b370eddfd66d67b4e4865b177729a78af5b27af2))
- **parser:** handle backslash escapes, autolinks, empty code ([`317b150`](https://github.com/jolars/panache/commit/317b150a07783e6b58c8f5de770c2da354af165b))
- **parser:** allow space after atx and any length setext ([`647d274`](https://github.com/jolars/panache/commit/647d2741bc95fcc901b831f26b2de3135b70d4f0))
- **parser:** enable `all_symbols_escapable` for commonmark ([`04c52d7`](https://github.com/jolars/panache/commit/04c52d7a20e0047c618a69f5b38e46f0f379df45))
- **formatter:** ensure blankline before header in commonmark ([`fd96f2a`](https://github.com/jolars/panache/commit/fd96f2a016d8b3177122d8734bdb96b3db9188dd))
- handle thematic breaks in commonmark correctly ([`f98fca0`](https://github.com/jolars/panache/commit/f98fca002c517d06a67c443d4c1e841ebe087842))
- **parser:** fix image link handling in commonmark ([`cac6004`](https://github.com/jolars/panache/commit/cac600484142950a97f77a3f3cf0cb8a67e2f21d))
- **parser:** preserve entity references in cmark ([`0ae7579`](https://github.com/jolars/panache/commit/0ae75793f54e59402a4d69f601b449ef681b7e25))
- **parser:** handle ATX headings in commonmark correctly ([`8c09c19`](https://github.com/jolars/panache/commit/8c09c19565292b363fafb1a08fd85a42c721d10d))
- **parser:** add extensions to commonmark flavor ([`59166ab`](https://github.com/jolars/panache/commit/59166ab00fc960b19a259ad31397eb50d541f69c))
- **parser:** don't emit synthethic token ([`a137fc4`](https://github.com/jolars/panache/commit/a137fc4d6352890a44ff47c247072be90077e8a0)), closes [#235](https://github.com/jolars/panache/issues/235)

### Dependencies
- updated crates/panache-formatter to v0.4.0
- updated crates/panache-parser to v0.6.0
## [2.40.0](https://github.com/jolars/panache/compare/v2.39.0...v2.40.0) (2026-04-27)

### Features
- **linter:** add adjacents footnote lint ([`9729418`](https://github.com/jolars/panache/commit/972941810bc18fe251b606e9d82c46e790952f7d)), closes [#229](https://github.com/jolars/panache/issues/229)

### Bug Fixes
- **parser:** include `~` in set of escapables ([`cfc0bfc`](https://github.com/jolars/panache/commit/cfc0bfcd5cf1e02fd7ef16b712d666df61e260b6)), closes [#231](https://github.com/jolars/panache/issues/231)
- **parser:** handle consecutive footnote definitions ([`e694627`](https://github.com/jolars/panache/commit/e694627654c497b66328d6062aa392af7337ce34))
- **linter:** gate lints by extensions ([`1a03e9b`](https://github.com/jolars/panache/commit/1a03e9b6b838602bc0fcce5b68d22b04ceca773c))
- **linter:** de-duplicate linter diagnostics ([`9c63e7e`](https://github.com/jolars/panache/commit/9c63e7ec8759759b9677c844528f82f12875c17d))
- **linter:** de-duplicate parsing calls, avoid extra parses ([`45407fe`](https://github.com/jolars/panache/commit/45407fedf049c2b127d40c6c4df7263af4a8cad1))

### Dependencies
- updated crates/panache-parser to v0.5.1
## [2.39.0](https://github.com/jolars/panache/compare/v2.38.0...v2.39.0) (2026-04-27)

### Features
- **cli:** make `--debug` actually useful in release builds ([`92a54ec`](https://github.com/jolars/panache/commit/92a54ecc087a10347a94fccfb7210dfdc345220f))
- **cli:** add `--quiet` command to suppress output ([`78818a1`](https://github.com/jolars/panache/commit/78818a18caace649d8eb064c1d5530c78b69a4e4)), closes [#221](https://github.com/jolars/panache/issues/221)
- **cli:** lint and format in parallel ([`c7560a0`](https://github.com/jolars/panache/commit/c7560a0fc3b68b694c7193dcb1a871537be1f1d4))

### Bug Fixes
- **formatter:** avoid quote character collisions ([`3c04c34`](https://github.com/jolars/panache/commit/3c04c3406eb4c84d1e1ef9a4dfe4051b33a6d111)), closes [#225](https://github.com/jolars/panache/issues/225)
- **cli:** disable color mode when `TERM=dumb` ([`eb8f12a`](https://github.com/jolars/panache/commit/eb8f12ab66e77d894cfe7c6b4488dfadcfb6f643)), fixes [#222](https://github.com/jolars/panache/issues/222)
- **parser:** emit empty cells for degenerate cells ([`095ada7`](https://github.com/jolars/panache/commit/095ada7da13f020de9856ae0ac06d2d441d451cd)), fixes [#224](https://github.com/jolars/panache/issues/224)

### Dependencies
- updated crates/panache-formatter to v0.3.0
- updated crates/panache-parser to v0.5.0
## [2.38.0](https://github.com/jolars/panache/compare/v2.37.0...v2.38.0) (2026-04-24)

### Features
- **cli:** default to global cache dir ([`52ed047`](https://github.com/jolars/panache/commit/52ed0470c8e7b8041a907bda5500a99a3acfeb47)), closes [#207](https://github.com/jolars/panache/issues/207)

### Bug Fixes
- **formatter:** don't break display math inside emphasis ([`d2eee34`](https://github.com/jolars/panache/commit/d2eee343d1e5099ca28a7a7dec50fb4aa9ca5f0b)), closes [#214](https://github.com/jolars/panache/issues/214)
- **formatter:** handle nested lists with continuation ([`185fa02`](https://github.com/jolars/panache/commit/185fa022db7e4c231bfddbe6efd01062033e948a)), closes [#212](https://github.com/jolars/panache/issues/212)
- **parser:** don't let definition list adopt trailing list ([`b2fba48`](https://github.com/jolars/panache/commit/b2fba48ab289b077a8d98c55152c61be7c978aa1))
- properly parse and format blockquote markers in deflist ([`b27eeb7`](https://github.com/jolars/panache/commit/b27eeb77aaf833aba1ab1370504b90b8a6e2d252)), closes [#209](https://github.com/jolars/panache/issues/209)
- **lsp:** only complete real `@`-type references ([`0419b45`](https://github.com/jolars/panache/commit/0419b45d2d42e2ad9cfe2a2019336d4937df1849))
- **formatter:** strip whitespace from code in list ([`b1b60c0`](https://github.com/jolars/panache/commit/b1b60c0e6e39b12d3143fee605a68b9057310f23))
- **parser:** allow Rcpp as known language in hahspipe parse ([`0fd5979`](https://github.com/jolars/panache/commit/0fd5979634810bbe2c42c238657b37b161d237a2))
- **lsp:** don't lowercase bib entries ([`2ee14d9`](https://github.com/jolars/panache/commit/2ee14d911931ec54cc33c7ac8d0c9efe5533fcbe))
- **formatter:** allow `Rcpp` as language for clang-format ([`521e087`](https://github.com/jolars/panache/commit/521e087f6dece4d09210249dc96c2db915b4975e))
- **linter:** correctly normalize bookdown equation label ([`dd7b736`](https://github.com/jolars/panache/commit/dd7b736bece825a27f122d586b9db3af85f74825))
- **lsp:** harden go-to-definition/reference triggers ([`16a9364`](https://github.com/jolars/panache/commit/16a936414b5425b5e44a6d5654a5cb6d3604ef12))
- **linter:** don't mix references with chunk labels ([`077db86`](https://github.com/jolars/panache/commit/077db86eac08e8766c200ff9e8b16479632a2743))
- handle UTF-8 boundary bug in table parsing ([`2c4e20f`](https://github.com/jolars/panache/commit/2c4e20f1039f97468879d083d87a878a09f79d96)), closes [#211](https://github.com/jolars/panache/issues/211)
- **parser:** correctly emit blanklines in tables/captions ([`0465f45`](https://github.com/jolars/panache/commit/0465f45dc437a7b8e0c751e672bc85e3806320d8)), closes [#210](https://github.com/jolars/panache/issues/210)
## [2.37.0](https://github.com/jolars/panache/compare/v2.36.0...v2.37.0) (2026-04-22)

### Features
- **formatter:** place table captions after the table ([`7d38d60`](https://github.com/jolars/panache/commit/7d38d604b314d2fb5645aea77fc34b1c2d23bdc7))
- **formatter:** use hanging indent for table captions ([`1234626`](https://github.com/jolars/panache/commit/1234626bce03c7e725426934ef5c289867e53137))
- **formatter:** use `:` as table caption prefix ([`618326a`](https://github.com/jolars/panache/commit/618326a97a5f1c2c178a2e2f508516f15b3d58d0))
- **formatter:** force one blankline after hashpipe options ([`68bba1b`](https://github.com/jolars/panache/commit/68bba1bec56cb0473a1de4b86c0f26f698a5f3fb)), closes [#115](https://github.com/jolars/panache/issues/115)

### Bug Fixes
- **parser:** don't parse caption as definition ([`e542c1f`](https://github.com/jolars/panache/commit/e542c1f59c3917feb885153590574eb22677818d))
- greedily consume table captions ([`58afc1c`](https://github.com/jolars/panache/commit/58afc1c2c27182a7e9768a1ff3f3b2b6e82531d5))
- **parser:** handle empty lines in hashpipe normalizer ([`51e6146`](https://github.com/jolars/panache/commit/51e614637bcd003f9970a546c540eaa92e0c3ea1)), closes [#201](https://github.com/jolars/panache/issues/201)
- **parser:** don't drop adjacent table caption ([`9144d63`](https://github.com/jolars/panache/commit/9144d636480e422378b929d0e03dd60cd31a719a)), closes [#200](https://github.com/jolars/panache/issues/200)
- **lsp:** handle nagivation URI correctly on windows ([`63f317e`](https://github.com/jolars/panache/commit/63f317e754e7362266a45cb9479d2e7c9536fdb1))
- **formatter:** correctly handle blanklines in blockquote ([`834757c`](https://github.com/jolars/panache/commit/834757c21a2844c27b46312a5a0ee0a7a003cc0d)), fixes [#199](https://github.com/jolars/panache/issues/199)
- **formatter:** handle blank line before fenced code ([`e7337fd`](https://github.com/jolars/panache/commit/e7337fdb4cece3a1cab45047b910cb43ac51efbc)), closes [#198](https://github.com/jolars/panache/issues/198)
- **formatter:** strip trailing whitespace in hashpipe flow ([`9757c2f`](https://github.com/jolars/panache/commit/9757c2fd16542f777e28c1cce3ce2b07e4f98d4d)), fixes [#194](https://github.com/jolars/panache/issues/194)
- **parser:** correctly parse deep list in blockquote ([`51484ac`](https://github.com/jolars/panache/commit/51484ac9b640278ea9eff860db6857cdcf07a931)), closes [#195](https://github.com/jolars/panache/issues/195)
- **formatter:** quote ambiguous labels in hashpipe conversion ([`e473944`](https://github.com/jolars/panache/commit/e4739441e3443dc8f6f50174bea14897a6b16f9a)), closes [#192](https://github.com/jolars/panache/issues/192)
- avoid wrapping on fancy markers in unsafe contexts ([`4de13dd`](https://github.com/jolars/panache/commit/4de13dd0fe44b9bb728d7aa22b772a2267cf060b)), closes [#193](https://github.com/jolars/panache/issues/193)
- **formatter:** handle citation spacing correctly ([`543aa46`](https://github.com/jolars/panache/commit/543aa46cc0ebbe3073e1eeda01b04bb058cd9d66)), ref [#193](https://github.com/jolars/panache/issues/193)
- **formatter:** don't collapse whitespace in hashpipe yaml ([`5d4b5d2`](https://github.com/jolars/panache/commit/5d4b5d2f60ef85a0ba557c62804795bd22f6f378)), closes [#185](https://github.com/jolars/panache/issues/185)
- **parser:** handle varying indentation for blockquotes ([`cdd3eec`](https://github.com/jolars/panache/commit/cdd3eec2c4b555476ed96d5c02dfd3a056876e86)), closes [#186](https://github.com/jolars/panache/issues/186)
- **formatter:** add list markers to unsafe wrappers ([`a7f1ed5`](https://github.com/jolars/panache/commit/a7f1ed514e33d956ca6892f9e6bf005f7c08ce6a)), closes [#187](https://github.com/jolars/panache/issues/187)
- **formatter:** normalize scalars to avoid idempotency issue ([`da9e3a0`](https://github.com/jolars/panache/commit/da9e3a0117bd152a1bb5407212168f0ed0640b17)), closes [#189](https://github.com/jolars/panache/issues/189)
- **parser:** accept empty headings ([`d081dd7`](https://github.com/jolars/panache/commit/d081dd72b5537b55ccb047879732ebf51df6ee4c))
- **parser:** properly handle adjacent tables ([`6206623`](https://github.com/jolars/panache/commit/6206623319b1a545fceedc67f5f6fa2596d9c1d8))
- **parser:** don't treat `:` table caption as def list ([`a287631`](https://github.com/jolars/panache/commit/a287631f90a0707b337f1d4438bb4bb9f8a28475))
- **parser:** handle bare URI in gfm flavor properly ([`2559a99`](https://github.com/jolars/panache/commit/2559a9958f70b4ba17abedc20a4c20bc85779053)), closes [#197](https://github.com/jolars/panache/issues/197)
- **parser:** fix logic around `blank_before_header` ([`c8f48c9`](https://github.com/jolars/panache/commit/c8f48c9ad69d3a3780a1a6ef2b300af203960eed))
- **parser:** handle bare `#|` comments ([`1a7d009`](https://github.com/jolars/panache/commit/1a7d009e08a964b059aae40241f70e28b30c5639)), fixes [#188](https://github.com/jolars/panache/issues/188) and [#190](https://github.com/jolars/panache/issues/190)
## [2.36.0](https://github.com/jolars/panache/compare/v2.35.0...v2.36.0) (2026-04-19)

### Features
- support smart punctuation ([`926a4c8`](https://github.com/jolars/panache/commit/926a4c80ed854f5a0afdfdae4d512adf91840525)), closes [#182](https://github.com/jolars/panache/issues/182)
- fallback to latest available release ([`008fe36`](https://github.com/jolars/panache/commit/008fe36721b32b92650fc79f441d5b867d7ea24d))

### Bug Fixes
- avoid special normalization of yaml and hashpipe items ([`d8bfb76`](https://github.com/jolars/panache/commit/d8bfb760e457d31bbec3ccebb4fb2089940a9377))
- **formatter:** handle list-in-blockquote idempotency issue ([`3d20ce4`](https://github.com/jolars/panache/commit/3d20ce4b198e7eadeccf071f76751f4898501f01)), closes [#177](https://github.com/jolars/panache/issues/177)
- handle idempotency in hashpipe yaml reconstruction ([`b28d675`](https://github.com/jolars/panache/commit/b28d675595ea314d18ce20bc0f50c2da45fc497f)), closes [#172](https://github.com/jolars/panache/issues/172)
- **parser:** parse display math over paragraph boundary ([`b5c9be2`](https://github.com/jolars/panache/commit/b5c9be2fc8d685df46bcf7cc81625337df53b029)), closes [#176](https://github.com/jolars/panache/issues/176)
- **parser:** handle utf-8 slicing in inline spans ([`8ccfe5c`](https://github.com/jolars/panache/commit/8ccfe5cee410162c84f85053528b5f829dc85c81)), closes [#175](https://github.com/jolars/panache/issues/175)
- **parser:** flush list-item inline buffer ([`a49179b`](https://github.com/jolars/panache/commit/a49179b14dbb6e753c2a2505a19df8c4e1d80afa)), closes [#174](https://github.com/jolars/panache/issues/174)
- **parser:** enable `inline_link` for GFM flavor ([`8059792`](https://github.com/jolars/panache/commit/805979269e898a4f28faddd15dcd07f2593f37ab)), closes [#171](https://github.com/jolars/panache/issues/171)
- update cargo lock file ([`3fc4d9b`](https://github.com/jolars/panache/commit/3fc4d9bf9c6a33af44de24fbcfc92e0843345a84))

## [2.35.0](https://github.com/jolars/panache/compare/v2.34.0...v2.35.0) (2026-04-16)

### Features
- add info-level debugging logs for external formatters ([`6228b55`](https://github.com/jolars/panache/commit/6228b5528121b2c2c27f51f1778b7a3e7bf024e4))

### Bug Fixes
- switch back to `v*` tagging for main program ([`9adc923`](https://github.com/jolars/panache/commit/9adc923ba70f396464cacf90dcaf50678a8c03b7))
- **parser:** handle utf-8 properly ([`92da1cd`](https://github.com/jolars/panache/commit/92da1cd74108f1576a846287ee3c098c04614b1d)), closes [#164](https://github.com/jolars/panache/issues/164)
- **lsp:** remove extra space in tasklist-bullet list action ([`6cc80b3`](https://github.com/jolars/panache/commit/6cc80b3eb8dd52a3b6e97a09133a8f54e905fb72))
- **lsp:** convert to actual task list ([`233f47c`](https://github.com/jolars/panache/commit/233f47c9bd73f4b6db6bcbe0ea52cd10da75ddb3))
- **parser:** handle utf-8 properly ([`92da1cd`](https://github.com/jolars/panache/commit/92da1cd74108f1576a846287ee3c098c04614b1d)), closes [#164](https://github.com/jolars/panache/issues/164)
- switch back to `v*` tagging for main program ([`9adc923`](https://github.com/jolars/panache/commit/9adc923ba70f396464cacf90dcaf50678a8c03b7))
- switch back to `v*` tagging for main program ([`9adc923`](https://github.com/jolars/panache/commit/9adc923ba70f396464cacf90dcaf50678a8c03b7))

## [2.34.0](https://github.com/jolars/panache/compare/panache-v2.33.1...panache-v2.34.0) (2026-04-14)


### Features

* allow auto-flavor for `.{R,r}markdown` ([27f3877](https://github.com/jolars/panache/commit/27f38777974426077937d91755be6c5cd9802f82))
* **linter:** add rule for unused labels ([a9d88d1](https://github.com/jolars/panache/commit/a9d88d150ddee9c3c9ef93516f88fb205e0d4430))
* **linter:** consider references across project ([2b687dc](https://github.com/jolars/panache/commit/2b687dc137db986710214eb6735ad9dcc4ae7e70))
* **lsp:** support go-to-references for footnote definition ([3d6e9f8](https://github.com/jolars/panache/commit/3d6e9f8a133dd4577effb11ba6a408bec3c1b599))


### Dependencies

* The following workspace dependencies were updated
  * dependencies
    * panache-parser bumped from 0.2.1 to 0.3.0

## [2.33.1](https://github.com/jolars/panache/compare/panache-v2.33.0...panache-v2.33.1) (2026-04-14)


### Bug Fixes

* **formatter:** avoid reflow-induced reparsing ([388b288](https://github.com/jolars/panache/commit/388b28841643b0af9f2e215e482942fe7b40b2b0)), closes [#134](https://github.com/jolars/panache/issues/134)
* **formatter:** prevent reinterpreting parse avoiding wrap ([fbf7733](https://github.com/jolars/panache/commit/fbf7733cba9d49a47b116e20e3297697bd36f501))
* **parser:** handle deep indentation and roman nos in list ([04b80f5](https://github.com/jolars/panache/commit/04b80f56f09801a9cfa1449c0f5e39670c9b6cfe)), closes [#143](https://github.com/jolars/panache/issues/143)
* **parser:** handle deep roman list and quotation ([b7aac81](https://github.com/jolars/panache/commit/b7aac81dc67bd38a04238d047d2b4c23d1214992)), closes [#137](https://github.com/jolars/panache/issues/137)
* **scripts:** correctly resolve tag in installation scripts ([5b474fc](https://github.com/jolars/panache/commit/5b474fc6abf0de21b3a2a192796297eab3eb88fa))


### Reverts

* "chore: don't include component in tag for root release" ([eb9e15f](https://github.com/jolars/panache/commit/eb9e15fd76ec601c1d01100218f804a36fdcdceb))


### Dependencies

* The following workspace dependencies were updated
  * dependencies
    * panache-parser bumped from 0.2.0 to 0.2.1

## [2.33.0](https://github.com/jolars/panache/compare/panache-v2.32.0...panache-v2.33.0) (2026-04-13)


### Features

* **cli:** add `--report` argument for `debug` command ([55f3489](https://github.com/jolars/panache/commit/55f3489272956e0dd593afdc72f03e62fd7d9db6))
* **formatter:** normalize non-breaking spaces to `\ ` ([8c1756b](https://github.com/jolars/panache/commit/8c1756bd4f3b2865f1e8e70a1091428b9652a75f))


### Bug Fixes

* **cli:** change `RUFF_CACHE_DIR` to `PANACHE_CACHE_DIR` ([644480b](https://github.com/jolars/panache/commit/644480b4ba7e2866bd662677329717d579055d12))
* **formatter:** don't allow `([@sec](https://github.com/sec))` to wrap to new line ([215a9c1](https://github.com/jolars/panache/commit/215a9c19ba5ce2fefb65470680891142250e1ba8)), closes [#138](https://github.com/jolars/panache/issues/138)
* **formatter:** fix block quote marker leakage into emphasis ([b5deeb3](https://github.com/jolars/panache/commit/b5deeb3e2feedb141f41db70b0ced2a18a3c6b22))
* **formatter:** improve citation and hashpipe handling ([768c741](https://github.com/jolars/panache/commit/768c741fcd30134ed19f6e27f5bdf2ffe32bacdc))
* **formatters:** use proper extensions for external format ([9fd9ab9](https://github.com/jolars/panache/commit/9fd9ab9cbcb9fe860341ba210f900847089d9376))
* **parser:** fix continuation detection in indented context ([4f1e51d](https://github.com/jolars/panache/commit/4f1e51d7fd0b8cc795747b95f3c223826832c9d7)), closes [#139](https://github.com/jolars/panache/issues/139)
* **parser:** fix losslessness bug in grid table parsing ([28f47dd](https://github.com/jolars/panache/commit/28f47dd0f66873fc092551520f9a356f038a431f)), closes [#132](https://github.com/jolars/panache/issues/132)
* **parser:** fix missing whitespace in nested fenced code ([426aa87](https://github.com/jolars/panache/commit/426aa87d70bf6b4ca7cf79853cdf2cf557e498de))
* **parser:** mitigate infinite recursion in line block ([612dc80](https://github.com/jolars/panache/commit/612dc80fc8adeeadcfe72ebf82ac332e00236347))
* **parser:** mitigate UTF-8 panic in hashpipe path ([26c702d](https://github.com/jolars/panache/commit/26c702dd0f66f8e3e36a7476e813eea3bc5ab2ee)), closes [#135](https://github.com/jolars/panache/issues/135)
* **parser:** preserve nonbreaking spaces in parser ([8c1756b](https://github.com/jolars/panache/commit/8c1756bd4f3b2865f1e8e70a1091428b9652a75f))
* **parser:** properly handle blandlines inside display math ([1e37724](https://github.com/jolars/panache/commit/1e377246d634c75abfcb9c77f7a142dd6d8e82ac)), closes [#130](https://github.com/jolars/panache/issues/130)


### Performance Improvements

* **parser:** move inline math tracking into container stack ([5df8308](https://github.com/jolars/panache/commit/5df8308a7f840ad22f86c254668b7725c9a4d03a))


### Reverts

* "chore(release): release 2.33.0 [skip ci]" ([01ac037](https://github.com/jolars/panache/commit/01ac037dc55b39ddcda83f5243e5e3a0192314fd))
* "ci: update smoke test" ([93c2ae9](https://github.com/jolars/panache/commit/93c2ae99fd39efd253c1644f7037689f72e54847))


### Dependencies

* The following workspace dependencies were updated
  * dependencies
    * panache-parser bumped from 0.1.0 to 0.2.0

## [2.32.0](https://github.com/jolars/panache/compare/v2.31.0...v2.32.0) (2026-04-09)

### Features

* **config:** more presets and add metadata ([8623393](https://github.com/jolars/panache/commit/86233937384c9e0e90db1517381d22f7963b8c80))
* **editors:** add Zed extension ([c87e45d](https://github.com/jolars/panache/commit/c87e45da91dc856f60c5fd558511e732363218c4)), closes [#102](https://github.com/jolars/panache/issues/102)
* **formatter:** contextual abbreviaton rules for wrapping ([1cf1a57](https://github.com/jolars/panache/commit/1cf1a57b7caf4b4f95d470b589397a58ec3af7f2))
* **formatter:** indent tables by two spaces ([864cc25](https://github.com/jolars/panache/commit/864cc251cb83086b107c7ed72d148b01d5134840)), closes [#89](https://github.com/jolars/panache/issues/89)
* **formatter:** presets for sqlfmt, alejandra, etc ([75f3df3](https://github.com/jolars/panache/commit/75f3df3e7440bbbe7c7f2ba00f3c12a08c1dbe74))
* **formatter:** reinstate wrapping in footnotes ([6bc8be1](https://github.com/jolars/panache/commit/6bc8be13e2b3b0f4b075862661154a238eb064bf))
* **formatter:** wrap table captions ([84a7fd1](https://github.com/jolars/panache/commit/84a7fd1d1b60f3c389b6a4e4962867ed6f02d191))
* implement caching for linting and formatting ([db37713](https://github.com/jolars/panache/commit/db3771361f59e6c4ce2761a483a7798944e29251))

### Bug Fixes

* **cli:** unify missing linters/formatters ([20c06c7](https://github.com/jolars/panache/commit/20c06c727b79d01b61c86f2770dd344953b84b66))
* **formatter:** fix idempotency issue with lists and images ([9a4c5ae](https://github.com/jolars/panache/commit/9a4c5aeeec143728e507619478eca7ba537bf862))
* **formatter:** handle idempotency issues in YAML, divs ([100f94c](https://github.com/jolars/panache/commit/100f94c46cc4b6c9c6499b9ddd4e253a181f3770))
* **formatter:** handle inline footnote newline wrapping ([b65a1f2](https://github.com/jolars/panache/commit/b65a1f29ceba4de39eb570f796c646fb97188954))
* **formatter:** setup proper rules for sentence wrapping ([4227f3a](https://github.com/jolars/panache/commit/4227f3a5079c311498f366f845787bdda34a9f4d)), closes [#113](https://github.com/jolars/panache/issues/113)
* **formatter:** treat code as non-breakable in sentence wrap ([482fe61](https://github.com/jolars/panache/commit/482fe6109ad677444b07615f1c6f246d0d2973a7))
* **formatter:** use pandoc blankline normalization in divs ([8491e97](https://github.com/jolars/panache/commit/8491e97936858294e6e9c21483fe9dd18f763efb))
* **formatting:** slacken non-breaking code logic ([131bc56](https://github.com/jolars/panache/commit/131bc56d239f1efee77bbef0e1d2c8812efd8a7b))
* **linter:** pass explicit shell dialect for shellsheck ([2d8c065](https://github.com/jolars/panache/commit/2d8c0658607aa221d0b61d27b9d521cb74f774ce))
* **parser:** handle complex fenced div case ([34e6e8c](https://github.com/jolars/panache/commit/34e6e8cae92836b66c998cc03a19ca3f92f5cd9a))
* workaround ambiguous fenced div idempotency ([19409ce](https://github.com/jolars/panache/commit/19409cea9be2bbb47ca2457364d8d4ce1a2a0a6a))

## [2.31.0](https://github.com/jolars/panache/compare/v2.30.0...v2.31.0) (2026-04-02)

### Features

* **linter:** add ruff as external linter ([a0506b6](https://github.com/jolars/panache/commit/a0506b655b62d792e444e13767427749b2929819))
* **linter:** add support for eslint external linter ([4d51e97](https://github.com/jolars/panache/commit/4d51e9770f34bbd7d03934f6a7d2a5d55519860b))
* **linter:** attach notes to external lints ([954dbd9](https://github.com/jolars/panache/commit/954dbd97da2c62cd428d55320394a59513a14cd9))
* **linter:** restrict external linters by language ([92a812c](https://github.com/jolars/panache/commit/92a812c97c9d43b4488112a92d603c08e1254582))
* **linters:** support shellsheck as external linter ([6875102](https://github.com/jolars/panache/commit/6875102689d40c8e939806513dc683a035400353))
* **linter:** support clippy as external linter ([db3340f](https://github.com/jolars/panache/commit/db3340f6b8bd916b89c8e19ceb9ff1e276dc7ca5))
* **linter:** support shellsheck as external linter ([e84c323](https://github.com/jolars/panache/commit/e84c3238881d6b008c7252c3fc618bf7d44ef9f8))
* **linter:** support staticcheck as external linter ([d665b04](https://github.com/jolars/panache/commit/d665b046bc4cfc0612e1af295fceb84c4636432c))
* **lsp:** add source action for linting ([01722b3](https://github.com/jolars/panache/commit/01722b3dfae018a49b70f75203fb0573684f30a0))
* **lsp:** use exact mappings for code action on ext linter ([5b905ef](https://github.com/jolars/panache/commit/5b905ef2db5381a1940c96442c15152a193adc9e))

### Bug Fixes

* **formatter:** honor wrapping mode in lists ([7fbba26](https://github.com/jolars/panache/commit/7fbba26b096829e7f5f50e7facb68deaea437a01)), closes [#103](https://github.com/jolars/panache/issues/103)
* **linter:** gate notes on linter path (external/internal) ([498ec8a](https://github.com/jolars/panache/commit/498ec8ab43329948613d300c365322552dccd164))

## [2.30.0](https://github.com/jolars/panache/compare/v2.29.0...v2.30.0) (2026-04-01)

### Features

* **lsp:** add hover preview for equations ([ea5f8b0](https://github.com/jolars/panache/commit/ea5f8b02533f1d0678bade3be395bb3ce90fe251))
* **parser,lsp:** support bookdown equation references ([bb1946b](https://github.com/jolars/panache/commit/bb1946b24d5568357ce9497aba87e956235aea07))

### Bug Fixes

* **formatter:** preserve non-breaking spaces ([e0861db](https://github.com/jolars/panache/commit/e0861db6983b4a0a60ac1362cb3c459e263adba7))
* **linter:** canonicalize absolute path to project root ([8362c9e](https://github.com/jolars/panache/commit/8362c9e038051dabea4502bed3cc67240541bbff))
* **parser,formatter:** handle inline executable code ([a2ba2f9](https://github.com/jolars/panache/commit/a2ba2f9ae38fea310985d5c525ceb7291a7f53d2))
* **parser:** catch headings after yaml with no blankline ([ba61c32](https://github.com/jolars/panache/commit/ba61c321f7353afb82e9a16627c446644e2ced51))
* unify cross-reference resolution ([c324753](https://github.com/jolars/panache/commit/c32475325258f846562decc9751529648b29d615))

## [2.29.0](https://github.com/jolars/panache/compare/v2.28.0...v2.29.0) (2026-03-28)

### Features

* **formatter:** separate top-level lists with blankline ([777e090](https://github.com/jolars/panache/commit/777e09054fca428a2c4c29da205ebbc0a0a1b795))
* **lsp,config:** add suppor for `gfm-auto-identifiers` ([31736da](https://github.com/jolars/panache/commit/31736dae549e4d5f45f6821c6e51e24c6c6e0805))
* **lsp:** add file renaming for linked documents ([8a7d08a](https://github.com/jolars/panache/commit/8a7d08a443f426b5cf900cac42177bb7a1391a5e))
* **lsp:** add hover support for for heading references ([3ca2b24](https://github.com/jolars/panache/commit/3ca2b248449e6b7cb4bd77620c308b035ac273c3))
* **lsp:** add hover support for linked markdown files ([4d88705](https://github.com/jolars/panache/commit/4d8870557fb86f474adc135aa6676b84395a824b))
* **lsp:** add hover support for reference definitions ([16bdd6f](https://github.com/jolars/panache/commit/16bdd6f90d9f48fdb564bd7a0cf182d4166ad6e7))
* **lsp:** add rename handler for footnote ids ([42be81d](https://github.com/jolars/panache/commit/42be81d211d4db110d020e3123743bd07b85bb3f))
* **lsp:** code action for converting bullet/ordered lists ([d620006](https://github.com/jolars/panache/commit/d620006aae3fd595ef79f66854c80920235cadcb))
* **lsp:** code action for converting list to task list ([f04e91e](https://github.com/jolars/panache/commit/f04e91edc5150ee6f2f9f85a07199bd275b2b7ee))
* **lsp:** extend file rename to shortcodes and nav elements ([492877f](https://github.com/jolars/panache/commit/492877f5eb5c443b798919d7204695cf11a2ba3c))
* **lsp:** support go-to-definition for example lists ([b7fbc19](https://github.com/jolars/panache/commit/b7fbc1996b5865221816b6aad9d128acf5ed5381))
* **lsp:** support renaming for example references ([15f6267](https://github.com/jolars/panache/commit/15f6267ef18e6903a577b1f879dea4a9bb54bb93))
* **parser:** parse yaml comments ([8b8e731](https://github.com/jolars/panache/commit/8b8e7316a4e0ef08af965e3369448baeb398f777))
* support `mmd-header-identifiers` extension ([ffe7834](https://github.com/jolars/panache/commit/ffe7834c67b1f85af911d7f4a897cdb759f40f77))
* support `mmd-link-attributes` extension ([5b44f9e](https://github.com/jolars/panache/commit/5b44f9ee804492b62f854b9856a153ef2a8ad589))
* support `mmd-title-block` extension ([276e31c](https://github.com/jolars/panache/commit/276e31c7f802497edfed768118fe4781f43c88f0))

### Bug Fixes

* **formatter:** handle idempotency failure with AUTO_LINK ([94c6c95](https://github.com/jolars/panache/commit/94c6c95634c3ad5735251baeebc6d49e2b26f897))
* **formatter:** mitigate idempotency growth in hashpipe yaml ([82a3da1](https://github.com/jolars/panache/commit/82a3da18c24b27e58f93207a87c4e2e069111891))
* **lsp,linter:** correctly resolve cross-reference to header ([c3a42cd](https://github.com/jolars/panache/commit/c3a42cd5ba349be1ef4b688bcbd8ff6f583b45ea))
* **parser:** actually enable `hard-line-breaks` extension ([70c9201](https://github.com/jolars/panache/commit/70c920186dab36b0ddd9dd665061a5b97d1b8253))
* **parser:** add missing code block extensions for gfm flavor ([1058df6](https://github.com/jolars/panache/commit/1058df6dc9d612420edf8de8eb9baff65b72b163))

## [2.28.0](https://github.com/jolars/panache/compare/v2.27.0...v2.28.0) (2026-03-25)

### Features

* **config:** gate executable code behind extension ([27a7e7e](https://github.com/jolars/panache/commit/27a7e7ee87aae5e2456fdc0aa630066f84bc53d3))
* **lsp:** add code action for converting to explicit link ([0b4a86c](https://github.com/jolars/panache/commit/0b4a86c7d918520da3939e88809da07bc762a1f5))
* **parser:** emit `CROSSREF_*` markers for crossrefs ([99d1174](https://github.com/jolars/panache/commit/99d1174b2cafa0b8131fbb39d0b61ac5f227a65c))
* **parser:** introduce bookdown-specific syntax tokens ([8a63e79](https://github.com/jolars/panache/commit/8a63e79dd115b878fedd7583e035173dd1b2589f))
* **wasm:** update the WASM library ([d004f73](https://github.com/jolars/panache/commit/d004f737a0ec653ec0f1f6adb37667331e15ddd5))

### Bug Fixes

* **formatter:** wrap and space blockquote in deflist ([22409e9](https://github.com/jolars/panache/commit/22409e9fa3cba43dc1ca2b86423d06f659b45b79))
* **lsp:** don't allow slugs in implicit links ([e7926fa](https://github.com/jolars/panache/commit/e7926fa4b66f9b551a8da4b05aab3b821e19a957))
* **lsp:** resolve go-to-headin for implicit links ([13fb0c4](https://github.com/jolars/panache/commit/13fb0c4770d67a730aa66953c6480597fa03ae4d))
* **parser:** emit list item buffer in nested def list ([9342463](https://github.com/jolars/panache/commit/9342463083ed9a102424763a2a072dbf6e6bc232))
* **parser:** handle nested lazy lists in definition lists ([7b32604](https://github.com/jolars/panache/commit/7b326044304e4689c66a875f6572cb6a00f0fe17))
* **wasm:** clean up and fix some warnings ([1261e92](https://github.com/jolars/panache/commit/1261e92db34af522f2afaaac15efa59366590a5c))

## [2.27.0](https://github.com/jolars/panache/compare/v2.26.0...v2.27.0) (2026-03-24)

### Features

* **cli:** don't error when running on glob or dir ([99e0440](https://github.com/jolars/panache/commit/99e0440dc78bdc34638259033bf44d074af8dc5e))
* **formatter:** format definition lists like pandoc ([8d96a40](https://github.com/jolars/panache/commit/8d96a40fa5515436897edb993f4c8d1f3f06800f))
* **formatter:** format nested headings ([75a74b3](https://github.com/jolars/panache/commit/75a74b3c28197f43a47a46b75a6747a9a4486192))
* **formatter:** normalize loose and compact definitions ([21b3c87](https://github.com/jolars/panache/commit/21b3c87e839bb7d88398bb177a7fb0a803b734c6))
* **lsp:** filter headings in document symbols ([c6af6bb](https://github.com/jolars/panache/commit/c6af6bb9bd9499b1a53af22b458cf5d3d1370bd3))
* **lsp:** introduce incremental parsing as experimental opt ([5be6df6](https://github.com/jolars/panache/commit/5be6df69c1e318c112a9e68765c7631010211726))

### Bug Fixes

* check LINK_REF first ([91ee195](https://github.com/jolars/panache/commit/91ee1954b4e63de498e29c537468b4360649be53))
* **config:** match includes/excludes relative to config root ([db2f95d](https://github.com/jolars/panache/commit/db2f95dae2049aa1f75d3977c08cc79a2ad00230))
* **parser:** encapsulate multiple definition list items ([e28fda6](https://github.com/jolars/panache/commit/e28fda648851be92af3dab67e25c9d2ce53e8826))
* **parser:** handle list as first lement in definition ([5871252](https://github.com/jolars/panache/commit/587125210dcce770bfdef70c2d227465b3b7176d))
* **parser:** parse headings in definition list items ([3aa9686](https://github.com/jolars/panache/commit/3aa96862a3aa2b3b94685bc2f2f084ccb870abcd))
* **parser:** parse headings in list items ([55e5632](https://github.com/jolars/panache/commit/55e5632186f32dd49757c127db85dd1de22f4088))
* **pre-commit:** remove pandoc submodule ([5baed02](https://github.com/jolars/panache/commit/5baed02690ecbd90db473dc454903f63de791f48)), closes [#92](https://github.com/jolars/panache/issues/92)

### Performance Improvements

* **lsp:** add section-based window incremental parsing ([99f7a0f](https://github.com/jolars/panache/commit/99f7a0f7203d53c3524e284bb31d98f69a647fb9))
* **lsp:** improve and document salsa durability policies ([7400e6e](https://github.com/jolars/panache/commit/7400e6e55b31273cb201a6fa58dd929688f7ee1f))
* **lsp:** improve fallbacks in incremental parsing ([c13fcb4](https://github.com/jolars/panache/commit/c13fcb43204b2f5290c3905e00f3e19f38e78be6))
* **lsp:** rollback non-working incremental parsing ([4dd29d4](https://github.com/jolars/panache/commit/4dd29d40adcbd18722a69ed6b4e80f82022c583d))

## [2.26.0](https://github.com/jolars/panache/compare/v2.25.0...v2.26.0) (2026-03-21)

### Features

* **formatter:** escape `]` ([f846ffc](https://github.com/jolars/panache/commit/f846ffc8bb30e3bb5c366dce92fd8b77a69570e1))
* **formatter:** standardize checkboxes to `[x]` ([59312ba](https://github.com/jolars/panache/commit/59312bad922669e8c7ded41e586505cc109d4f4d))
* introduce `pandoc-compat` field ([58d9e54](https://github.com/jolars/panache/commit/58d9e543481f17353225da674c413a6f49d23498))
* **lsp:** implement document links ([eb590e0](https://github.com/jolars/panache/commit/eb590e0489a59fdfbfb8ad0300d2b7a6407b1ce7))

### Bug Fixes

* **parser:** don't accept `[]` as tasklist check box ([8700911](https://github.com/jolars/panache/commit/8700911d6efaaf8c7373a609776506bf7a59ba13))
* **parser:** emit LINK_REF nodes for reference images ([127946d](https://github.com/jolars/panache/commit/127946de79f8c77754cd359011380a3e53efce46))

## [2.25.0](https://github.com/jolars/panache/compare/v2.24.1...v2.25.0) (2026-03-20)

### Features

* **cli:** add `--dump-passes` to `panache debug format` ([f54549e](https://github.com/jolars/panache/commit/f54549e4d182b5c9a2d58a2c2c4739a0e96662c0))
* **editors:** provide qmd and rmd languages in vscode ([da9bc5a](https://github.com/jolars/panache/commit/da9bc5ad6bc57ac554f67db6fc37e46e8a07a539))
* **formatter:** compress simple table columns to content ([98c4e8a](https://github.com/jolars/panache/commit/98c4e8aa7218eaf2d30a7fe106ee8e8a93c865c7))

### Bug Fixes

* **formatter:** don't interpret ````markdown as fenced code ([9e17ebc](https://github.com/jolars/panache/commit/9e17ebccd166d6a987d23a2d7b6fdd3b37fdc250))
* **formatter:** preserve markers in headerless table ([62ec59a](https://github.com/jolars/panache/commit/62ec59a6b5644ac15ce00ae6b5a4ef9dd0bf016b))
* **formatter:** recover lost indentation in code block ([2f94707](https://github.com/jolars/panache/commit/2f94707092c327468a76abcf3e544da6f97a1047))
* **formatter:** restrict yaml frontmatter replacement ([7e9bf72](https://github.com/jolars/panache/commit/7e9bf72523a416752e69fb9c411a514ccdcf71f1))
* **lsp:** correctly handle renaiming ([b5c0b5b](https://github.com/jolars/panache/commit/b5c0b5bb5f69f8abc402e4a9ae392d2c9be30a51))
* **lsp:** limit highlight of definition to actual label ([e377024](https://github.com/jolars/panache/commit/e37702404341f659a90591a1430180da61ebf759))
* **parser,formatter:** fix regression in definition list ([10a2cd7](https://github.com/jolars/panache/commit/10a2cd7cc0c5db59871fc9e338c2b166288b30eb))
* **parser:** don't treat continuation as code block ([927efeb](https://github.com/jolars/panache/commit/927efebfae35106cecfae67adfbc5b570a1f68ca))
* **parser:** fix losslessness bug in empty definition ([4ede2f8](https://github.com/jolars/panache/commit/4ede2f81ec29c2ebcb9ea7c81e05dc69fa695c24))
* **parser:** match pandoc's rules for list item identation ([c15688d](https://github.com/jolars/panache/commit/c15688d4fd6ec690c74e97d18311cf5cbccce814))

## [2.24.1](https://github.com/jolars/panache/compare/v2.24.0...v2.24.1) (2026-03-19)

### Bug Fixes

* **formatter:** account for prefix when formatting hashpipe ([b3471c2](https://github.com/jolars/panache/commit/b3471c2581ca82edd0c073f8c0decb618258898a))
* **formatters:** don't hide warnings behind log flag ([59b5dc0](https://github.com/jolars/panache/commit/59b5dc01a656767826c20e318bc7bd631e91123b))
* **lsp:** handle hyphenated references when renaming ([2d507b8](https://github.com/jolars/panache/commit/2d507b854988c4ed778c1fe3098f00f71ba1dd5c))

## [2.24.0](https://github.com/jolars/panache/compare/v2.23.0...v2.24.0) (2026-03-19)

### Features

* **formatter:** report missing external formatters jointly ([1e5cd25](https://github.com/jolars/panache/commit/1e5cd254d3156509cb5b583e5ef68578b8476372))
* **lsp:** add workspace heading symbols ([ac6a8d9](https://github.com/jolars/panache/commit/ac6a8d9f59c0a3d3079825a03860e80ae39ac277))

## [2.23.0](https://github.com/jolars/panache/compare/v2.22.0...v2.23.0) (2026-03-18)

### Features

* **config:** exclude `LICENSE.md` files by default ([8cdad49](https://github.com/jolars/panache/commit/8cdad49c136d89b34ba6b2287a110ca637fb9bc5)), closes [#80](https://github.com/jolars/panache/issues/80)
* **linter,lsp:** unify bookdown chunk label resolution ([a71301f](https://github.com/jolars/panache/commit/a71301f105364c421a193388209f7901545aec51))
* **linter:** warn on uncaptioned bookdown figure crossrefs ([2de688a](https://github.com/jolars/panache/commit/2de688a49cb2cafd2f1e9cf84f8f649fac280eac))
* **lsp,linter:** support bookdown-style divs ([d6f08af](https://github.com/jolars/panache/commit/d6f08af6464cb5cd6c7a513e2f8aaa4a6f73ba0a))
* **lsp:** add support for Pandoc heading links ([9690922](https://github.com/jolars/panache/commit/969092248c8b3a14da1ec3e2d89d550c4e382f5e))

### Bug Fixes

* **cli:** fix exit code with `--force-exclude` ([f140a7b](https://github.com/jolars/panache/commit/f140a7bd0248ba37c03eb5c52bef97531aa3d589)), closes [#82](https://github.com/jolars/panache/issues/82)
* **formatter:** enforce wrapping in list item ([c9266cb](https://github.com/jolars/panache/commit/c9266cb4501710785ad7c490f7b1c2574203bace)), closes [#81](https://github.com/jolars/panache/issues/81)
* **lsp:** correctly report heading symbols ([97d8bdb](https://github.com/jolars/panache/commit/97d8bdb15cbbc084e238d32c33db56d182234e6a)), closes [#84](https://github.com/jolars/panache/issues/84)
* **lsp:** handle bookdown crossrefs with dashes ([101e546](https://github.com/jolars/panache/commit/101e546e6f7c624739513db55794b38b6c14a71f))

## [2.22.0](https://github.com/jolars/panache/compare/v2.21.0...v2.22.0) (2026-03-17)

### Features

* add automatic installer scripts ([1f20e76](https://github.com/jolars/panache/commit/1f20e763874093a16e45ce369768b3cde7c4ec8c))
* add suppor for `tex_math_gfm` extension ([70e74cb](https://github.com/jolars/panache/commit/70e74cbaa7f4effc95353f8b3a1d5186a27f468e))
* **config:** add `extensions.<flavor>` ([2accb02](https://github.com/jolars/panache/commit/2accb02da95065d78a830c4e26791e166c985d25))
* **config:** add `flavor-overrides` config option ([6f54ff4](https://github.com/jolars/panache/commit/6f54ff42a70bc5440e2de9c3aed6971e01e19f9c))
* **formatter:** format horizontal rules to line-width ([4910606](https://github.com/jolars/panache/commit/49106063a936608b2367eb2ad56d2b4ed1f93c6f))

### Bug Fixes

* **formatter:** handle hashpipe YAML correct ([27b3df6](https://github.com/jolars/panache/commit/27b3df6c505a6007654b2ccd1fdbdcbf7b21c135))
* **formatter:** mitigate indentation infinite growth ([264e49c](https://github.com/jolars/panache/commit/264e49cb76af764550a82c135cb4952a85c81128)), closes [#78](https://github.com/jolars/panache/issues/78)
* **parser,formatter:** handle multiline exec options ([e19c8ed](https://github.com/jolars/panache/commit/e19c8ed48d6640fd928b5c66a74d56c675b04cf1))
* **parser:** don't parse horizontal rules as metadata/table ([b695b3d](https://github.com/jolars/panache/commit/b695b3d36103aad91aa9fcb634bb50fc773035e2))

## [2.21.0](https://github.com/jolars/panache/compare/v2.20.0...v2.21.0) (2026-03-16)

### Features

* build binaries for linux musl too ([d6ada87](https://github.com/jolars/panache/commit/d6ada875d04cd2152142300b29570a7439420851))
* build binaries for windows arm too ([05f8c46](https://github.com/jolars/panache/commit/05f8c460137e4536e8d0638add505a22c4b787a6))
* **cli:** add `--message-format <fmt>` for linter ([2eafc8c](https://github.com/jolars/panache/commit/2eafc8c7091bf80d234d970ca07323ad273688c9))
* **config:** add `[format]` as replacement for `[style]` ([c86ef90](https://github.com/jolars/panache/commit/c86ef90eef1cb55028d80e6029385b328782dd84))
* **config:** add include, exclude, extend-include/exclude ([0d3a05e](https://github.com/jolars/panache/commit/0d3a05ed48755d0b4a760b8ac9624add508cea55)), closes [#71](https://github.com/jolars/panache/issues/71)
* **config:** expose `auto-identifiers` extension ([bdf0081](https://github.com/jolars/panache/commit/bdf0081912a53a37bf7da45fe15e8671f148c01e))
* **config:** move rules to `[lint.rules]` category ([6fc9ade](https://github.com/jolars/panache/commit/6fc9ade2a56565172269afbd6db9b336f3517470))
* **formatter:** drop blanklines at start of document ([e784c3d](https://github.com/jolars/panache/commit/e784c3de6eb5fcd15ba9edb5d6978ee3d9dd99e8))
* **formatter:** remove code block config options ([3dd5846](https://github.com/jolars/panache/commit/3dd5846a47ed94f92771a78728d97045a4292515))
* **linter:** add contextual hint for heading hierarchy lint ([1ce7a18](https://github.com/jolars/panache/commit/1ce7a1870f8297ba931486a292c2e803fce18195))
* **linter:** improve lint display ([bd74591](https://github.com/jolars/panache/commit/bd74591473d60dff422d54f56ba7f59f7191c912))
* **lsp:** adapt project graphs to `project.render` settings ([be63ee9](https://github.com/jolars/panache/commit/be63ee9aefefd73b6f59ae45a2be23f7914430dc))
* **parser,linter:** add support for github emojis `:smile:` ([116fad2](https://github.com/jolars/panache/commit/116fad2effc0829d6af1a7575c5861ee321760a9))

### Bug Fixes

* **config:** correctly align GFM flavor with Pandoc ([7f151f8](https://github.com/jolars/panache/commit/7f151f87f012de7edbcd73a275c0f05e16fd358a))
* exclude release-assets from crate package to prevent crates.io 413 error ([#77](https://github.com/jolars/panache/issues/77)) ([34d8196](https://github.com/jolars/panache/commit/34d8196f1ecb4e57a1760e092051894ad57c02a9))
* fix problem with `--force-exclude` ([f77b670](https://github.com/jolars/panache/commit/f77b670f1fc8829c84de96823f3562513b1fecb8))
* fix relative path from root issue on macos ([22470ab](https://github.com/jolars/panache/commit/22470aba892160d6141999a120d7dcf783c77aab))
* **parser:** add multiple missing extensions guards ([b8e2e37](https://github.com/jolars/panache/commit/b8e2e37157058359412be47d8bfa006e8c6f7bd8))

### Performance Improvements

* **editors:** bundle vsix extension and use esbuild ([815635c](https://github.com/jolars/panache/commit/815635cf393581a72bba07ff9f486f0263e70c57))

## [2.20.0](https://github.com/jolars/panache/compare/v2.19.0...v2.20.0) (2026-03-13)

### Features

* **linter:** add linting rule for missing code chunk labels ([a8f4709](https://github.com/jolars/panache/commit/a8f4709ab943297a9912761cb9a6acff6a9fb07d)), closes [#68](https://github.com/jolars/panache/issues/68)
* **linter:** add rule for duplicate chunk labels ([50806ba](https://github.com/jolars/panache/commit/50806bad26cfd9a5d5262590f752380b2c973f6e))
* **lsp:** add find-references support for crossrefs ([475bd94](https://github.com/jolars/panache/commit/475bd94cca5e5fba61c7e17c7cadad5e89e21478))
* **lsp:** add go-to-def, rename for exec chunk labels ([5f4367d](https://github.com/jolars/panache/commit/5f4367db2c71557c49f5c040507f068094f72807))
* **lsp:** extend find-references to citations ([ec2d328](https://github.com/jolars/panache/commit/ec2d328170d04406d91570e411a4660424adc8eb))
* **parser:** parse in-comment execution options ([35c772d](https://github.com/jolars/panache/commit/35c772d0b469c88e12b3d272820fb53dcaa2bc9b))

### Bug Fixes

* **parser:** handle unicode properly ([5886d05](https://github.com/jolars/panache/commit/5886d05d5558271fa8daeb92c5b125cb4c68c265))

## [2.19.0](https://github.com/jolars/panache/compare/v2.18.0...v2.19.0) (2026-03-12)

### Features

* add support for github alerts ([31d8055](https://github.com/jolars/panache/commit/31d8055f092ca6daa55a9d12736075415d9217f9))
* **linter:** add linting rule for spaces in labels ([d8e522e](https://github.com/jolars/panache/commit/d8e522e4d70dc6a21de836652d26c17bf889af02))
* **linter:** add missing link references rule ([2232449](https://github.com/jolars/panache/commit/223244989f5b9c759b468b811af8bab3e6f6db66))

### Bug Fixes

* **formatter:** handle labels with spaces in them ([be100ae](https://github.com/jolars/panache/commit/be100ae46219a57e86fe73dcbd5eaabf9de6765e)), closes [#66](https://github.com/jolars/panache/issues/66)
* **lsp:** handle umlauts properly ([a8227fb](https://github.com/jolars/panache/commit/a8227fb3f8eb51b32427c4a9516b3cadc669c753)), closes [#65](https://github.com/jolars/panache/issues/65)
* **parser:** handle `---` without blankline before ([746d827](https://github.com/jolars/panache/commit/746d827c92f7f1234bab2b6aff063e6ba8d44681))

## [2.18.0](https://github.com/jolars/panache/compare/v2.17.0...v2.18.0) (2026-03-12)

### Features

* **cli:** add `--no-color` and `--isolated` ([f19b7f5](https://github.com/jolars/panache/commit/f19b7f5bdaf40eeb3e5e7d77a68a96a17fd9834b))
* **cli:** add `--stdin-filename` ([a574782](https://github.com/jolars/panache/commit/a5747827ec50c7fb47edbc158bf344fe1cb0e03e))

### Bug Fixes

* **formatter:** maintain idempotency with `  ` and `\\n` ([b22e91e](https://github.com/jolars/panache/commit/b22e91e47a3dfb116fcf0706ef10cd74c0339052))
* **formatter:** remove space in code block fences ([0a81b0f](https://github.com/jolars/panache/commit/0a81b0fd8e0d4675dcf447bc1b4dd60680294931))
* **parser:** parse `\cmd{\n<content>\n}` as `TEX_BLOCK` ([8373ffb](https://github.com/jolars/panache/commit/8373ffb48792f702531425eaafec52aec58c91f5))

## [2.17.0](https://github.com/jolars/panache/compare/v2.16.0...v2.17.0) (2026-03-11)

### Features

* **editors:** add VS code and Open VSX extensions ([#57](https://github.com/jolars/panache/issues/57)) ([0570c84](https://github.com/jolars/panache/commit/0570c8496feda8531ae9f64f8cc663f1ee2d88f7)), closes [#55](https://github.com/jolars/panache/issues/55)

### Performance Improvements

* **formatter:** use built-in greedy wrapper ([ac73a3a](https://github.com/jolars/panache/commit/ac73a3acb769f9babff6ea5cdffbba0fbf03426d))

## [2.16.0](https://github.com/jolars/panache/compare/v2.15.0...v2.16.0) (2026-03-11)

### Features

* **cli:** add `panache debug format` for debugging ([1319489](https://github.com/jolars/panache/commit/13194899f7c338e99924e272055510c9dd975080))
* **formatter:** use first-fit word wrapping ([66957be](https://github.com/jolars/panache/commit/66957be8fc08052b18f05edc079f1352180b32bf))

### Bug Fixes

* **build:** gate warnings behind `debug_assertions` ([71c1b24](https://github.com/jolars/panache/commit/71c1b24f1196a9f619ac7e51b73a8265f897a91d))
* **build:** use `InitializeResult` defaults, update lockfile ([e1b045e](https://github.com/jolars/panache/commit/e1b045ee12f30c56b1cf8358be68b34547b07ca2)), closes [#53](https://github.com/jolars/panache/issues/53)
* **formatter:** fix idempotency in emphasis formatting ([5e492a5](https://github.com/jolars/panache/commit/5e492a5535a908999f4cff64634afe60fa7ca189))
* **formatter:** fix idempotency issue in definition list ([04b2b7f](https://github.com/jolars/panache/commit/04b2b7fe73dab8c83d2e5ca4bca64f509ddad63c))

## [2.15.0](https://github.com/jolars/panache/compare/v2.14.1...v2.15.0) (2026-03-10)

### Features

* **formatter:** normalize indented tables ([c4b394f](https://github.com/jolars/panache/commit/c4b394f27cfb4a4b86db08db40c6374f8dfe72f0))

### Bug Fixes

* **formatter:** fix idempotency around table caption ([aad08f6](https://github.com/jolars/panache/commit/aad08f6d9d654fc47de5aab6e6610fd571724467))
* **formatter:** fix idempotency failure with display math ([d7e2b47](https://github.com/jolars/panache/commit/d7e2b47f5c21c9d6faed76f0fefbd34386fee2a1))
* **formatter:** fix idempotency issue in hard break in list ([1b46852](https://github.com/jolars/panache/commit/1b4685250a5345d42347d492b1939742f9240f86))
* **formatter:** fix idempotency issue with display math ([f47edc9](https://github.com/jolars/panache/commit/f47edc9a8336a49a2867cbaca7a38a5d99e0394e))
* **formatter:** handle footer and multirow grid tables ([821e54f](https://github.com/jolars/panache/commit/821e54f4e230439fc5fe521e414f03df2b2ad533))
* **formatter:** handle idempotency in code span formatting ([188d10f](https://github.com/jolars/panache/commit/188d10f7e14167493600be3aa68277a8249e28f1))
* **formatter:** handle idempotency with blockquote marker ([854b5fe](https://github.com/jolars/panache/commit/854b5feda5ceff37c2bbe1842137940cab36c744))
* **formatter:** handle tex blocks properly in formatter ([04ad902](https://github.com/jolars/panache/commit/04ad90267d914239c00fedb66b924d85b1dd07f7))
* **formatter:** preserve malformed display math with dollars ([78e2907](https://github.com/jolars/panache/commit/78e290790664053a7874ae0d4f5408f73fc03762))
* **formatter:** protect inline math spaces ([d6470b6](https://github.com/jolars/panache/commit/d6470b60ee64a52b815eb4b1acce34208c32e279))
* **parser,formatter:** handle consecutive tables ([f1a4c08](https://github.com/jolars/panache/commit/f1a4c08b056f5c28d0dafc36c967e85b86f17a8b))
* **parser,formatter:** harden grid table parsing ([05bdab9](https://github.com/jolars/panache/commit/05bdab946578e3d6061ec2dfa7ae55d0bf9f7c9a))
* **parser:** don't hardcode emphasis markers ([ce7125e](https://github.com/jolars/panache/commit/ce7125edafe3b56f7cce6cbbd700fcb3e01f8bf2))
* **parser:** parse whitespace after code block starter ([3d28e74](https://github.com/jolars/panache/commit/3d28e7430f45982d58b2dbb7da276d82bd8a7608))

## [2.14.1](https://github.com/jolars/panache/compare/v2.14.0...v2.14.1) (2026-03-10)

### Bug Fixes

* **formatter:** correct list idempotency ([3b0db0e](https://github.com/jolars/panache/commit/3b0db0e8936cef252bd2fb72563f6e1e1699fc9d))
* **formatter:** fix idempotency failure in atx headings ([6a61caf](https://github.com/jolars/panache/commit/6a61caf614803060d268aaaf48bc9076aa3f87e8))
* **formatter:** handle div in loose list ([6514e58](https://github.com/jolars/panache/commit/6514e58404417659baa654619f661cd517c5baad))
* **formatter:** handle escaped char inside table ([130df6f](https://github.com/jolars/panache/commit/130df6fc594fa2347b1c719e067018c74e23b1a5))
* **formatter:** handle horizontal before setext heading ([225d7b2](https://github.com/jolars/panache/commit/225d7b28b51a6e78d6fe0add77bcba5b96c35b10))
* **formatter:** handle non-ASCI able content ([4ea70f4](https://github.com/jolars/panache/commit/4ea70f4fdacb39e444d6dc10ce0803c992deca49))
* **formatter:** handle underscore emphasis with nested asterisks ([71f41b0](https://github.com/jolars/panache/commit/71f41b0b86c6d4c295ac563247d7bf0dfa63c245))
* **formatter:** subdue blockquote marker after hard break ([e3b53c9](https://github.com/jolars/panache/commit/e3b53c90060e25411c302ee8e37ecaff75908ce7))
* **parser,formatter:** tighten code fence logic ([9c1ffcc](https://github.com/jolars/panache/commit/9c1ffccca3c7ad3fffd8fa17f72598e9b1ee3824))
* **parser:** allow fenced blocks to interrupt paragraphs ([0e521b5](https://github.com/jolars/panache/commit/0e521b5b500e861f4664cd4e359400271cb49fcd))
* **parser:** allow references with leading spaces ([9051331](https://github.com/jolars/panache/commit/9051331e6338b4f8be248149468b49de1f9336d6))
* **parser:** avoid stealing captions as definition items ([22855d0](https://github.com/jolars/panache/commit/22855d0399fa4c7d80700c65d75aeacf18c2c391))
* **parser:** cater to spanning-style rows ([57e3ab3](https://github.com/jolars/panache/commit/57e3ab33f00c6cbeaec696946903b00500fcee89))
* **parser:** don't interpret continuation line as list ([af73bd4](https://github.com/jolars/panache/commit/af73bd446464106f39ebc917e56540af07f54cb6))
* **parser:** emit leading spaces before rule ([8d58381](https://github.com/jolars/panache/commit/8d58381ae7f07d24d59673b56601052461f379ac))
* **parser:** emit original line block marker ([0866449](https://github.com/jolars/panache/commit/0866449f0c47702f3c68f95f067554452611dbf6))
* **parser:** fix backtick-parsing in attributes ([5f82e22](https://github.com/jolars/panache/commit/5f82e22f08353a5e7cdad40606cb451c0633dc28))
* **parser:** handle table with complex layout ([47fd1a3](https://github.com/jolars/panache/commit/47fd1a3b67f75ad8790621e2255cbf67b4800526))
* **parser:** honor `blank-before-header` extension ([c1f3571](https://github.com/jolars/panache/commit/c1f3571f026ddb44a2562b8b0e2d06261a67f226))
* **parser:** preserve leading whitespace before fences ([7f12c62](https://github.com/jolars/panache/commit/7f12c628e3719a69082c88617d750660483c7af3))
* **parser:** relax fence block detection ([6cc356d](https://github.com/jolars/panache/commit/6cc356d4bfb7ba98bfbc658bd746a3415c872393))

## [2.14.0](https://github.com/jolars/panache/compare/v2.13.0...v2.14.0) (2026-03-09)

### Features

* **cli:** add `--quiet` flag ([47ee630](https://github.com/jolars/panache/commit/47ee630362745f742c8cbe9566257905d90fcad6))
* **cli:** add `--verify` for format and parser ([f8fd6e6](https://github.com/jolars/panache/commit/f8fd6e6819e348393f92ce03a25a15d779be34e3))
* **cli:** make `--verify` a pure smoke-test screen ([3619207](https://github.com/jolars/panache/commit/3619207469d5e2b579f370f2514c4118cd246e7e))
* **formatter:** don't treat semicolons as sentence break ([ade76a9](https://github.com/jolars/panache/commit/ade76a93212ec7bfd93509a07581ba1cfac8996f)), closes [#48](https://github.com/jolars/panache/issues/48)

### Bug Fixes

* **formatter:** apply block code formatting inline ([5d76bea](https://github.com/jolars/panache/commit/5d76bea933011846baf8d4cd483e28927ddbb8dd))
* **formatter:** don't line break after initials ([3030451](https://github.com/jolars/panache/commit/30304517cf3bf6525e74c40083b35ec6f26527f7))
* **formatter:** fix idempotency in fancy list formatting ([f5c6509](https://github.com/jolars/panache/commit/f5c6509e0ba36c3fddc4b5f4940f0aaf5278c76d))
* **formatter:** handle crossref in blockquote ([2b4e729](https://github.com/jolars/panache/commit/2b4e729b9519fa79e694751d3eda642acb521342))
* **formatter:** handle empty cells in grid tables ([ecc7515](https://github.com/jolars/panache/commit/ecc7515154f9b68c7749039c28d3ecce8ddda52d))
* **formatter:** harden external formatting ([2946761](https://github.com/jolars/panache/commit/2946761627fefb9b68e71947af4720a8f42a35d4))
* **formatter:** require blankline before line block ([0589776](https://github.com/jolars/panache/commit/0589776a73192000839fd1dce687b9917ab74159))
* **parser:** correctly parse trailing `#` ([942c1fa](https://github.com/jolars/panache/commit/942c1fad07fd907996f57b3e5fa99624b2ea9e8c))
* **parser:** don't drop trailing whitespace in fenced div ([7bd2d31](https://github.com/jolars/panache/commit/7bd2d31c469d1e3c8dfea1deb9846a679de950cf))
* **parser:** don't require blankline before fenced div ([f17c3aa](https://github.com/jolars/panache/commit/f17c3aa6fc7ae7e8e61a869f813236fea1fb1877))
* **parser:** don't trim trailing space in definition ([edeae6f](https://github.com/jolars/panache/commit/edeae6f42a1d9e886c0f378e43f7985b518f1e3d))
* **parser:** handle line block inside grid table ([100ebed](https://github.com/jolars/panache/commit/100ebed0b3e30506c6f36a8f92d4654c6e1d4aee))
* **parser:** handle list inside blockquote ([e20e756](https://github.com/jolars/panache/commit/e20e75661aff09f738d703dac8fb446f2c26d8dd))
* **parser:** handle rows exceeding separator width ([4a42c63](https://github.com/jolars/panache/commit/4a42c6383e47746c1606d83bf01e9256ca15c780))
* **parser:** handle shortcode in heading ([200bfd8](https://github.com/jolars/panache/commit/200bfd829fe4af175286e53878bc86c4bfc283a2))
* **parser:** handle spaces in indented code block ([cdbf952](https://github.com/jolars/panache/commit/cdbf952b105e0b8f485f40855a317f1b443ec59e))
* **parser:** handle table after div close ([a4c2940](https://github.com/jolars/panache/commit/a4c2940f294b56aedc59f1cd3143bc4bf57be40c))
* **parser:** handle trailing whitespace in grid table ([0677abb](https://github.com/jolars/panache/commit/0677abb8f2d7605760dcb5c3ae95e708eef456c4))
* **parser:** handle unicode in shortcode ([7f603dc](https://github.com/jolars/panache/commit/7f603dc7b0cc7d24a3bcbaf204f18b73aa32d171))
* **parser:** parse indented block in block quote losslessly ([bbd2f86](https://github.com/jolars/panache/commit/bbd2f869c2265813b539bc336b7f5c7d48e297a7))
* **paser:** don't trim trailing whitespace after marker ([32e9734](https://github.com/jolars/panache/commit/32e97342a2de92bc2f375df60c8db95cbaa91775))

### Performance Improvements

* **lsp:** add lazy definition and hover handling ([69f7cce](https://github.com/jolars/panache/commit/69f7cceadc8ac27de6593462aafe760ddc5a5f03))
* **lsp:** add LRU tuning ([7a5d439](https://github.com/jolars/panache/commit/7a5d43945cb885513f9117fc65481d77cac1e572))
* **lsp:** derive lint and metadata diagnostics through salsa ([4ede8cb](https://github.com/jolars/panache/commit/4ede8cb872697d7de3b95b85cf9bca6a6b139b0b))
* **lsp:** introduce durability macros into graph ([b74248e](https://github.com/jolars/panache/commit/b74248ea6c02dcd96d748e2c1b5773266932f112))
* **lsp:** unify lint pipeline to avoid duplicate parse ([070e7f5](https://github.com/jolars/panache/commit/070e7f54fc1f86111cd6ef40b46d633549ac41b4))
* **lsp:** use `salsa::interned` for project graph intternally ([996be36](https://github.com/jolars/panache/commit/996be360c5e6df449c31cb9c20d59260beb2a73e))

## [2.13.0](https://github.com/jolars/panache/compare/v2.12.0...v2.13.0) (2026-03-07)

### Features

* **formatter:** add `tab-width` setting ([3e02336](https://github.com/jolars/panache/commit/3e023369ad5853de80c47325d8a94f7324e4fc95))
* **formatter:** normalize spacing inside fenced div ([6aa73d0](https://github.com/jolars/panache/commit/6aa73d046bd96c1b37e9506832c0bc1edfd89c04))
* **formatter:** wrap multiline footnote refs as Pandoc ([722c76a](https://github.com/jolars/panache/commit/722c76acc974b66542be8fb1a34974c77ec5b097))
* **lsp:** add `--debug` flag ([ad5d81a](https://github.com/jolars/panache/commit/ad5d81a090cca6f12802c5f6d3bae639621401c5))
* **parser:** add support for raw tex blocks ([841a663](https://github.com/jolars/panache/commit/841a6637dcd2f2357e89274445d7216f7811e824))

### Bug Fixes

* **formatter:** fix wrapping for definition lists ([4dd084b](https://github.com/jolars/panache/commit/4dd084b36cf00acf0296862b6ddb45703313a844))
* **formatter:** omit quarto/knitr comments from formatting ([36ceccb](https://github.com/jolars/panache/commit/36ceccba17531817db4f7014730c3114232e68ef))
* **formatter:** use correct ruff args ([408d330](https://github.com/jolars/panache/commit/408d3307362d537d31392ab67bd0a0e6c976ee5d)), closes [#46](https://github.com/jolars/panache/issues/46)
* **linter:** mitigate spurious warning for quarto crossrefs ([a0e0769](https://github.com/jolars/panache/commit/a0e076929780c631ffbcd25a17e0c82cad79b267))
* **lsp,linter:** correct bib file found range, deduplicate ([9d5dfbb](https://github.com/jolars/panache/commit/9d5dfbba272ff1e105b23b854ccbf84a3fef7ee2))
* **parser,formatter:** align with pandoc's fenced div parse ([1982972](https://github.com/jolars/panache/commit/1982972ee509f591922383c6780dacc81f573557))
* **parser:** fix infinite recursiong bug in tex cmd parse ([1f71833](https://github.com/jolars/panache/commit/1f718334ca981486200fbe61942db380c5652973))
* **parser:** handle tab stops gracefully ([9f8aa96](https://github.com/jolars/panache/commit/9f8aa96aacabd5e94039bf2e53deeea0ccd518f6))
* **parser:** only accept four spaces-indented def lists ([11fb109](https://github.com/jolars/panache/commit/11fb109cf93c28cfa668c3f5b8e9020fea153a89))

### Performance Improvements

* **lsp:** build graph lazily ([0efcc0d](https://github.com/jolars/panache/commit/0efcc0d7898de35780e9d73ae77e4df248a258d3))
* **lsp:** cache bibliography data ([edecc10](https://github.com/jolars/panache/commit/edecc106b3eb17684d1a3fcf15c8994477ed30d5))

## [2.12.0](https://github.com/jolars/panache/compare/v2.11.0...v2.12.0) (2026-03-05)

### Features

* add RIS bibliography support ([128eaf0](https://github.com/jolars/panache/commit/128eaf0b9baee65a7a4d2e58af912ae704a4f13c))
* **formatter,linter:** support ignore directives ([17a3df2](https://github.com/jolars/panache/commit/17a3df2a8306b8330acf4b5ab952589cc08a849c))
* **formatter:** add blanklines between definitions if loose ([c6a3d14](https://github.com/jolars/panache/commit/c6a3d144d6ef071ce92a3fb01c302e9689410969))
* improve hover preview for citations ([45e0f11](https://github.com/jolars/panache/commit/45e0f11bbed0d6d9cd14047d1106a9a596d0a355))
* support JSON bibliographies ([3a9ee26](https://github.com/jolars/panache/commit/3a9ee26f4186d3ef0531cbbc6dccc9eb17ac5f3e))

### Bug Fixes

* fix compilation error ([194858a](https://github.com/jolars/panache/commit/194858acf577426944974de5f81de4330ca9d6d8))
* **formatter:** handle indentation in indented code blocks ([9112856](https://github.com/jolars/panache/commit/911285687aa0bc45ade15b767ae5fdbd32f67f74))
* handle code block on first line of definition item ([4bb42f5](https://github.com/jolars/panache/commit/4bb42f5b75ecd5691cb211bc08e6e68b704eea05))
* **lsp:** expand selection for edit range to top-level block ([0a39399](https://github.com/jolars/panache/commit/0a393990dac59bf44e4f46316f831dd13464bd06))
* **lsp:** improve expansion handling for range formatting ([11c4d51](https://github.com/jolars/panache/commit/11c4d51eb49d37f01aa90e999e5ab628453c917e))
* **lsp:** replace correct segment when using range format ([5968b6a](https://github.com/jolars/panache/commit/5968b6a1ae0f8bc737dfe2d218f4857e1f255931))
* **parser, formatter:** correctly handle blocks in deflist ([4ffc8bc](https://github.com/jolars/panache/commit/4ffc8bc42facad1cf8b5b02f82152b769ccc7c56))
* **parser,formatter:** handle loose/compact definitions ([063f9f3](https://github.com/jolars/panache/commit/063f9f36b90c9a5b101d9cd2951ddb456cf37868)), closes [#45](https://github.com/jolars/panache/issues/45)
* **parser:** don't treat indented lists and code blocks ([7b14077](https://github.com/jolars/panache/commit/7b140778e3bf278aee14ce0f465210f7ab45b3c7))
* **parser:** require blankline before list in definition ([ac971c0](https://github.com/jolars/panache/commit/ac971c0d90727750ea70e0df7bb06b7274b97bdf))
* resolve bibliography paths relative to metadata files ([3a878bc](https://github.com/jolars/panache/commit/3a878bc385977f3af9a6cc2a53ebb14714a2a978)), closes [#44](https://github.com/jolars/panache/issues/44)

## [2.11.0](https://github.com/jolars/panache/compare/v2.10.0...v2.11.0) (2026-03-04)

### Features

* add support for implicit header references ([d9fe4a3](https://github.com/jolars/panache/commit/d9fe4a368cd3e81d9a703a50279b3ea0cf974c8a))
* **formatter:** add preset for clang-format ([d3f2a60](https://github.com/jolars/panache/commit/d3f2a600282200bfa9e1cc3ad4b63d3d1bb62bce))
* **formatter:** add preset for shfmt ([83143a2](https://github.com/jolars/panache/commit/83143a207ef295535785a97e6c5654e16b04e28f))
* **formatter:** add preset for taplo TOML formatter ([d5b83e5](https://github.com/jolars/panache/commit/d5b83e50f4daf3dfafc4ab7a3709273e23f1ba1f))
* **lsp,linter:** add support for inline YAML references ([08c141d](https://github.com/jolars/panache/commit/08c141d2d22a641dc12c4dbda9ed2eaae417f476))
* **lsp,linter:** enable bookdown project integration ([315bc50](https://github.com/jolars/panache/commit/315bc500ab12d1b86b04dafcd5bb58a7e8a47cc6))
* **lsp,linter:** support diagnostics and more for includes ([15b61fc](https://github.com/jolars/panache/commit/15b61fcfd9f89a88e07d327b09613dda2bab08f6))
* **lsp,linter:** use project and metadata files ([3ed27fb](https://github.com/jolars/panache/commit/3ed27fbbbc5309a85a476065c466a62e103d9c6b))
* **lsp:** add go-to-def handler for crossrefs ([35c2a06](https://github.com/jolars/panache/commit/35c2a06e676f84234f4085707a26614aff7e94ee))
* **lsp:** add renaming support for bibliography entries ([7bb30d0](https://github.com/jolars/panache/commit/7bb30d0ea0c28ae75ccd3886e010e73c7f6f8d3f))
* **lsp:** handle quarto cross-references separately ([086e6ed](https://github.com/jolars/panache/commit/086e6edb69d907c94cf9683e510b1bc7c218593b))
* **lsp:** maintain project-wide state ([6ea5356](https://github.com/jolars/panache/commit/6ea53567e8959e0759b3db97efb7b4d8ec51bceb))
* **parser:** support bookdown crossref syntax ([45ef2eb](https://github.com/jolars/panache/commit/45ef2ebeed2538970fb4389419f0fdd6b61bd3fc))

### Bug Fixes

* **formatter:** handle equation attributes with line after ([eecf1a5](https://github.com/jolars/panache/commit/eecf1a54d2895d0fbce56eefca9d6e9fa0255ce8))
* **lsp,linter:** deduplicate bibliography entries ([6602569](https://github.com/jolars/panache/commit/6602569a4d924c9a50551de92b2e9b87cdc9c962))
* **lsp:** fix duplicate bibliography issue ([7f85ff7](https://github.com/jolars/panache/commit/7f85ff7bcb44bf7e5ef07a5318c3f00bbb39bcad))
* **lsp:** show correct lines for bib diagnostics ([30177ae](https://github.com/jolars/panache/commit/30177ae85ee53048b45b761509a2545d8c3caaa8))
* **lsp:** use platform-independent file Uris ([658c3a4](https://github.com/jolars/panache/commit/658c3a44d1197e6f4ca153a8bf956aebbf6b7cfc))
* **lsp:** use platform-independent URIs ([2aecf8e](https://github.com/jolars/panache/commit/2aecf8ebfe7cf3f41d20999ee47537cad520c82e))
* **parser, formatter:** don't wrap latex commands ([619dea5](https://github.com/jolars/panache/commit/619dea50b6c26d8396d898fa1a4e255eaa0f9230))

## [2.10.0](https://github.com/jolars/panache/compare/v2.9.0...v2.10.0) (2026-03-03)

### Features

* **formatter:** add sentence-wrapping mode ([4048f55](https://github.com/jolars/panache/commit/4048f555cf28178027170f9aef4d4d86948a832c))
* **linter,lsp:** add auto-fixing for external linters ([f73e3be](https://github.com/jolars/panache/commit/f73e3be6beb9ddc444a06a2aa7bc6cb587674164))

### Bug Fixes

* **lsp,linter:** return correct range for bibliography lint ([313ca32](https://github.com/jolars/panache/commit/313ca323a450fc04f5d105c3cbf296e5d2bab3e5))
* **lsp:** add external lint fixing code action ([1e5a847](https://github.com/jolars/panache/commit/1e5a8474dca8f96e6254adb3fd321d537917ba90))
* **lsp:** fix go-to-definition and hover handlers for citations ([ef7d5e7](https://github.com/jolars/panache/commit/ef7d5e7e06a398e4dbc2e3f18f3af3b34af3efc3))
* **lsp:** handle go-to-definition for references ([7a0bc17](https://github.com/jolars/panache/commit/7a0bc175fe46a4ed126864244738d25cc785fc42))

## [2.9.0](https://github.com/jolars/panache/compare/v2.8.0...v2.9.0) (2026-03-02)

### Features

* **formatter:** normalize links to match pandoc ([3b5fdce](https://github.com/jolars/panache/commit/3b5fdce1a97670bd58f18f2257d04cc9d6bdd4e1))

### Bug Fixes

* handle list inside fenced div ([6f1014c](https://github.com/jolars/panache/commit/6f1014c7df892ca60e1b55885f95ca628670c16d))
* **lsp:** correctly extract text in AST wrappers ([9bacf4d](https://github.com/jolars/panache/commit/9bacf4d943801f49cf1adfabe5c83d8c4570dfd5))
* **lsp:** correctly map external lints to buffer ([4bef1b3](https://github.com/jolars/panache/commit/4bef1b31d4d90ec94a1251498cf9c7f5dbcc84ca))

## [2.8.0](https://github.com/jolars/panache/compare/v2.7.0...v2.8.0) (2026-03-02)

### Features

* **cli:** add `--json` option to parse ([c84ce49](https://github.com/jolars/panache/commit/c84ce495e1af98f34af0ccaea70aa0872fb6a933))
* **config:** consistently use kebab-case ([b01b5b1](https://github.com/jolars/panache/commit/b01b5b1768eefb5379fb10b25e44a78c0921af8f))
* **lsp:** add support for external bibliographies ([47d5177](https://github.com/jolars/panache/commit/47d51776caa7d8aba6372a04236d65e9d9295fcb))
* **parser:** handle CLRF line endings in bibtex parser ([0d8a2c8](https://github.com/jolars/panache/commit/0d8a2c8c5975dfaab2d82787acc014a6b3e9ac02))

### Bug Fixes

* correctly parse and format inline code spans with `s ([7a6336b](https://github.com/jolars/panache/commit/7a6336be417512fe1e1de92b6fcabcfaca3f0233))
* **parser:** correctly parse CRLF newline at end ([af31e51](https://github.com/jolars/panache/commit/af31e516c1c1013647cf24418dfb2b8d2c2484f7))
* **parser:** handle UTF-8 correctly in citation parsing ([4678265](https://github.com/jolars/panache/commit/46782655609d884919eed8916c39017f2c3a868b))
* **parser:** handle whitespace after heading and before attr ([ee230ef](https://github.com/jolars/panache/commit/ee230ef1a5d989f317fe413161cd367c83168037))

## [2.8.0](https://github.com/jolars/panache/compare/v2.7.0...v2.8.0) (2026-03-02)

### Features

* **cli:** add `--json` option to parse ([c84ce49](https://github.com/jolars/panache/commit/c84ce495e1af98f34af0ccaea70aa0872fb6a933))
* **config:** consistently use kebab-case ([b01b5b1](https://github.com/jolars/panache/commit/b01b5b1768eefb5379fb10b25e44a78c0921af8f))
* **lsp:** add support for external bibliographies ([47d5177](https://github.com/jolars/panache/commit/47d51776caa7d8aba6372a04236d65e9d9295fcb))
* **parser:** handle CLRF line endings in bibtex parser ([0d8a2c8](https://github.com/jolars/panache/commit/0d8a2c8c5975dfaab2d82787acc014a6b3e9ac02))

### Bug Fixes

* correctly parse and format inline code spans with `s ([7a6336b](https://github.com/jolars/panache/commit/7a6336be417512fe1e1de92b6fcabcfaca3f0233))
* **parser:** correctly parse CRLF newline at end ([af31e51](https://github.com/jolars/panache/commit/af31e516c1c1013647cf24418dfb2b8d2c2484f7))
* **parser:** handle UTF-8 correctly in citation parsing ([4678265](https://github.com/jolars/panache/commit/46782655609d884919eed8916c39017f2c3a868b))
* **parser:** handle whitespace after heading and before attr ([ee230ef](https://github.com/jolars/panache/commit/ee230ef1a5d989f317fe413161cd367c83168037))

## [2.7.0](https://github.com/jolars/panache/compare/v2.6.3...v2.7.0) (2026-03-01)

### Features

* add pre-commit hook configuration ([b31ecdb](https://github.com/jolars/panache/commit/b31ecdb503fdc880552d9a0f76a41a99d31eb838)), closes [#37](https://github.com/jolars/panache/issues/37)

### Bug Fixes

* handle complex blocks in blockquotes ([ec69e51](https://github.com/jolars/panache/commit/ec69e518ee91fb1f94b594ff8593b86a4ee92d6f))
* **parser:** fix bug in losing blockquote marker ([403165b](https://github.com/jolars/panache/commit/403165bddc9029401cd43291e242ecd398bfb3f3))

### Performance Improvements

* **lsp:** add incremental parsing ([b804ee9](https://github.com/jolars/panache/commit/b804ee947c2d5f6a2c753b256cd234670607923d))

## [2.6.3](https://github.com/jolars/panache/compare/v2.6.2...v2.6.3) (2026-02-27)

### Performance Improvements

* refactor parser into block dispatcher approach ([#36](https://github.com/jolars/panache/issues/36)) ([4804f80](https://github.com/jolars/panache/commit/4804f806d64eea4ebaf852aeead6703422e238fc))

## [2.6.2](https://github.com/jolars/panache/compare/v2.6.1...v2.6.2) (2026-02-27)

### Bug Fixes

* **parser:** handle multilines in blockquotes ([02d7c20](https://github.com/jolars/panache/commit/02d7c204515f276420da5aa229cb581b0616d199))
* reimplement support for setext headings ([12c9182](https://github.com/jolars/panache/commit/12c91829ac0eb4f66e47c57071c208b45e504670))

## [2.6.1](https://github.com/jolars/panache/compare/v2.6.0...v2.6.1) (2026-02-25)

### Bug Fixes

* **parser:** handle complex emphasis cases ([f7fe514](https://github.com/jolars/panache/commit/f7fe51439e81da6ae3a838c7ab7c8a91eb3dfc9c))

## [2.6.0](https://github.com/jolars/panache/compare/v2.5.1...v2.6.0) (2026-02-20)

### Features

* **config:** add `[style]` section, deprecate old version ([2b83231](https://github.com/jolars/panache/commit/2b83231fb98db153f442268a4613a6a63aa6f6d6))
* **config:** add `append_args` and `prepend_args` ([56cb4c1](https://github.com/jolars/panache/commit/56cb4c10debdcbf784e284d5cea953e7ab3307b5))
* **config:** allow partial overrides ([d53e1d0](https://github.com/jolars/panache/commit/d53e1d0c7c59f2a580a0806de34d985aa1c98e16))
* **config:** flavor-independent code block styling ([5c14f2f](https://github.com/jolars/panache/commit/5c14f2f4173c9beee5f89724bcd5c38c38dce486))
* **config:** remove pointless `min_fence_length` ([4204ed5](https://github.com/jolars/panache/commit/4204ed5d21aebdb8644c9e37f5e35aa60eedca26))
* **config:** remove unused `normalize_indented` ([da087e4](https://github.com/jolars/panache/commit/da087e4d7245aa753c9b87fe6270759100c4ffa3))
* **config:** use `[formatters.<formatter]` style ([7d91023](https://github.com/jolars/panache/commit/7d91023527f2704213b26b496e42f5484a11efbf))
* **formatter:** don't assume `#|` for unknown language ([b50f3ab](https://github.com/jolars/panache/commit/b50f3aba386431c3c4757482867213e70ee83075))
* **formatter:** format simple tables ([5d048c6](https://github.com/jolars/panache/commit/5d048c6de1daa8c20864a4af967bd4b5f9fbdc02))
* **formatter:** support ojs, mermaid, dot in hashpipe conversion ([8695ae2](https://github.com/jolars/panache/commit/8695ae2ea99f1e54ad838c7e342b9b0cd82518b4))
* **formatter:** trim trailing blanklines ([6e7cd61](https://github.com/jolars/panache/commit/6e7cd614e8f3a9373ff8e0017a05227beba65916))
* **linter:** add rule for duplicate references ([97fbc8a](https://github.com/jolars/panache/commit/97fbc8ab7dfbda1f4ac567e4586dbcb4c6286101))
* **lsp:** add convert to loose/compact list code action ([a63c104](https://github.com/jolars/panache/commit/a63c104d3199bef7aa2c35f0a575d38daaf6fabe))
* **lsp:** convert between footnote styles ([2fe5030](https://github.com/jolars/panache/commit/2fe50308a1dfa27a2268b8b0af44f814801fbdc2))
* **lsp:** enable footnote preview on hover ([d25c74a](https://github.com/jolars/panache/commit/d25c74a09f39efbb25360b60fcb8d829166f1c1b))
* **parser:** drop `ROOT` node from AST tree ([6c9bd8f](https://github.com/jolars/panache/commit/6c9bd8f1ffc8c480d8adf435b23b981072acae7a))
* **parser:** parse `](` in links and images ([73a8da0](https://github.com/jolars/panache/commit/73a8da0a02cee020470edf052b2805bb76197c41))
* update wasm build ([ff6acd9](https://github.com/jolars/panache/commit/ff6acd9cf40d2c16bba6b88de17f8db32ac02ff1))

### Bug Fixes

* **config:** override code block flavor defaults ([4023e29](https://github.com/jolars/panache/commit/4023e29ca64ae19cd070cb062a16996c33e28ab7))
* **formatter:** concatenate successive blanklines ([5e1c06a](https://github.com/jolars/panache/commit/5e1c06a5b568e8b00ef48746707d3615b15b31fb))
* **formatter:** correct alignment in multline tables ([04c9ad6](https://github.com/jolars/panache/commit/04c9ad6d5625af89b5624617fdf545ffca59e817))
* **formatter:** fix idempotency issue in table formatting ([fe4af95](https://github.com/jolars/panache/commit/fe4af958915a4c1c17fcadc0d2b157eaf68d9194))
* **formatter:** handle attributes correctly in code blocks ([6228182](https://github.com/jolars/panache/commit/6228182e192cf58293de5d22d6cdc495a3a2591a))
* **parser:** avoid parsing expressions ([69bea2b](https://github.com/jolars/panache/commit/69bea2b68a67b00846f6c14fc37bffbe8715979a))
* **parser:** correctly parse multiline captions before table ([c8389d4](https://github.com/jolars/panache/commit/c8389d47945d886472d641692bf40e9e46c71b4d))
* **parser:** don't parse links in `CODE_INFO` ([2f10b8b](https://github.com/jolars/panache/commit/2f10b8b8ec909ff585a19fc89a75c8c11cf7aa39))
* **wasm:** guard yaml formatter behind wasm flag ([063143c](https://github.com/jolars/panache/commit/063143cde61edaa877f2d1ba5e201667c08770f5))

## [2.5.1](https://github.com/jolars/panache/compare/v2.5.0...v2.5.1) (2026-02-18)

### Bug Fixes

* **formatter:** properly handle grid table alignments ([56c5bba](https://github.com/jolars/panache/commit/56c5bbae206fc1eb7bfc343724e2cb244258c67a))
* **parser:** fix issues with CRLF parsing ([6ec62f0](https://github.com/jolars/panache/commit/6ec62f07d7385549911ad90f0788dfd16393a413))

## [2.5.0](https://github.com/jolars/panache/compare/v2.4.0...v2.5.0) (2026-02-17)

### Features

* **parser:** parse compact and loose lists and use  `Plain` ([3258724](https://github.com/jolars/panache/commit/3258724c72268f45499b89bcf4290199c11a4380))
* **parser:** parse quarto equation references ([0ce1f7d](https://github.com/jolars/panache/commit/0ce1f7d9242cc6d85af045b9d3815ca53c24e17a))
* **parser:** parse shortcodes ([c6abc24](https://github.com/jolars/panache/commit/c6abc2479aca0267d5d8c9dedb40702d6e6f58e3))
* **parser:** rename BlockMathMarker to DisplayMathMarker ([68c9c32](https://github.com/jolars/panache/commit/68c9c32532e4c78016d6f870500c8ffb24053cb5))
* **parser:** standalone figures as `Figure` node ([59d74e7](https://github.com/jolars/panache/commit/59d74e7cdbe4434b52b127144dd1cc316aaeda40))

### Bug Fixes

* **config:** override flavor defaults ([8fe291b](https://github.com/jolars/panache/commit/8fe291b1c001b83ba7d74c7a0ec6ad2c4f0e151e))
* **formatter:** strip newline for external yaml format ([3d54b3e](https://github.com/jolars/panache/commit/3d54b3eaea79ae41f2fc76abfa3ab93a09e11a66))
* **parser:** correctly parse lists with different markers ([273ba39](https://github.com/jolars/panache/commit/273ba39c1c247073c83c6d2e66dbb058b26f7e2e))
* **parser:** handle lazy lists with blanklines ([9d82a92](https://github.com/jolars/panache/commit/9d82a92dd8ba2eeeb7cf84875164156c05042291))
* **parser:** parse blanklines away from plain nodes ([e7972ee](https://github.com/jolars/panache/commit/e7972ee46473ec37363ba2634488ccb339f96a4f))
* **parser:** parse display math if begin/ends on delim line ([ef16594](https://github.com/jolars/panache/commit/ef165947530a99ba32fe3eaf14c14461133e04bf))

## [2.4.0](https://github.com/jolars/panache/compare/v2.3.0...v2.4.0) (2026-02-15)

### Features

* **formatter:** format YAML metadata with ext formatters ([eb89f06](https://github.com/jolars/panache/commit/eb89f063f9135d0a9e18122ff63ca9742b421af4))
* **lsp:** emit warnings for missing bibliographies ([14fa9c9](https://github.com/jolars/panache/commit/14fa9c9eff1d1dd908b8ff3e34a6a080ddb68311))

### Bug Fixes

* **formatter:** wrap first lines in definition lists ([3ad7576](https://github.com/jolars/panache/commit/3ad75764c290c26ec445362f29f7ec5db3602aae))

## [2.3.0](https://github.com/jolars/panache/compare/v2.2.0...v2.3.0) (2026-02-14)

### Features

* **cli:** add support for external linters ([c1937de](https://github.com/jolars/panache/commit/c1937deeb58c3f816709dd01c9976f5e0c7d3bac)), closes [#23](https://github.com/jolars/panache/issues/23)
* **formatter:** add support for formatting grid tables ([ef47bac](https://github.com/jolars/panache/commit/ef47bac2c45e5e0d1e52341e20f440ca39ba5002))
* **lsp:** add go to definition for links, images, footnotes ([d749424](https://github.com/jolars/panache/commit/d74942480682e0cb82d86b30eeb9d7f4c931dea9))
* **lsp:** add support for external linters (just jarl for r now) ([5162096](https://github.com/jolars/panache/commit/516209697f9fe49e11bf6ec0e621f4a67f3dd466))
* **lsp:** implement `textDocment/foldingRange` ([7ce6ce2](https://github.com/jolars/panache/commit/7ce6ce27a4abe2df6c6e087a1bab0222a1ea3f38))
* **parser:** parse code block language as token ([c29016e](https://github.com/jolars/panache/commit/c29016e8ff56271d4b0f9e79abf582f6b29f8836))
* **parser:** preseve LF and CRLF line endings ([a470713](https://github.com/jolars/panache/commit/a47071378bc46ca49a3cf1c15f3aee5512749664))

### Bug Fixes

* **formatter:** handle unicode in table formatting ([44f4bcf](https://github.com/jolars/panache/commit/44f4bcff60c85d6b4f672bca0a6aedf8d22236fd))
* **formatter:** honor "line-ending" configuration option ([248e2f2](https://github.com/jolars/panache/commit/248e2f21fc3b89f3d02879a40a9ce860d144c235))
* **lsp:** correctly detect flavor in document symbols ([60af5b4](https://github.com/jolars/panache/commit/60af5b4b7b943857a25ed35afc63bf351316cf2e))
* **parser:** consistently handly CRLF line endings ([6b43c9c](https://github.com/jolars/panache/commit/6b43c9c54e70539ff3b3d51d4a26495e0a5219b9))
* **parser:** correctly parse captions before tables ([2cb9e2d](https://github.com/jolars/panache/commit/2cb9e2d6a8daf9ee08c70eb57702cfef7fc84622))
* **wasm:** fix wasm build by fixing command invocation ([a9a29a7](https://github.com/jolars/panache/commit/a9a29a7039b51efd41a5964496e609d1ed5b244a))

## [2.2.0](https://github.com/jolars/panache/compare/v2.1.0...v2.2.0) (2026-02-13)

### Features

* **cli:** format and lint multiple files, or by globbing ([f53a8fd](https://github.com/jolars/panache/commit/f53a8fdde164ec4348027e2969cec2e9b84eeedd))
* **formatter:** initial formatting of execution options ([879b291](https://github.com/jolars/panache/commit/879b291ae4255f0a2a1cf68d8bb19b2a96ea2cf4))
* **formatter:** normalize hard line breaks to escaped ([ada9f0f](https://github.com/jolars/panache/commit/ada9f0ffc9b1c46b88881b801cc906a33509290b))

### Bug Fixes

* correctly parse and handle escaped line breaks ([49154ff](https://github.com/jolars/panache/commit/49154ffde8d36ce549803012ae3f4caa6eecc769))
* **formatter:** handle content after opening math delim ([ef8c220](https://github.com/jolars/panache/commit/ef8c2202e1192da7acd246b804a6d5bbbe09ec88))
* **lsp:** auto-detect flavor from file extension ([84dc96f](https://github.com/jolars/panache/commit/84dc96f26bcfa06d76588d8ec2a7c7f368be2258))
* make parser lossless ([4add809](https://github.com/jolars/panache/commit/4add809613bbe5db15549e8cd061a4d09fd19ee9))
* **parser:** check for blank line in math after delim ([f65858e](https://github.com/jolars/panache/commit/f65858e3fa60f3d3d08551008314b605ca51fb76))

## [2.1.0](https://github.com/jolars/panache/compare/v2.0.0...v2.1.0) (2026-02-12)

### Features

* **lsp:** add initial support for document symbols ([81a7ef9](https://github.com/jolars/panache/commit/81a7ef9b1bab9adf336924856b5451a89b05ccaa))

### Bug Fixes

* don't wrap quarto/rmd code chunk args in quotes ([48ebd68](https://github.com/jolars/panache/commit/48ebd68669f474b9ce334eaedcb2936d078449c9)), closes [#22](https://github.com/jolars/panache/issues/22)

## [2.0.0](https://github.com/jolars/panache/compare/v1.0.0...v2.0.0) (2026-02-12)

### ⚠ BREAKING CHANGES

* change external formatting to be opt-in

### Features

* add presets for external formatters ([70b297a](https://github.com/jolars/panache/commit/70b297a70afa8a503984c130384df4a2e2b6ac1c))
* add range formatting ([902cb95](https://github.com/jolars/panache/commit/902cb95924bd2be53da403726ca5418e67da34dd))
* change external formatting to be opt-in ([8d91753](https://github.com/jolars/panache/commit/8d917536de3d8454ab68e4b53bdbdea643a6650c))
* **formatter:** standardize unordered lists to `-` marker ([33ae608](https://github.com/jolars/panache/commit/33ae60838e4fbe26b4877aba492981ec17e7b578))
* implement a linter ([4af0d5e](https://github.com/jolars/panache/commit/4af0d5ecb104da94841073967653e1e36740f6c3))
* implement wrapping for links and images ([929f993](https://github.com/jolars/panache/commit/929f9931e468891b08e9d05c3d387bd807bc501a))
* **lsp:** integrate linter with LSP server ([f0ae3e9](https://github.com/jolars/panache/commit/f0ae3e90778dfe9b8b6e495655ef0ab721089887))

### Bug Fixes

* correctly deal with nested lists in definitions ([5f00893](https://github.com/jolars/panache/commit/5f008930aa4459c0db20cb813509c5daf021c251))
* correctly delegate non-stdin formatters ([869d473](https://github.com/jolars/panache/commit/869d47316ffe49e98f891e462f82a83fe59cfc3d))
* correctly praser backslash-escaped math ([c28cdc5](https://github.com/jolars/panache/commit/c28cdc5cfa05fcacd6c851f3686d96e1c7166ab3))
* don't use defunct `--write` flag ([bbe3291](https://github.com/jolars/panache/commit/bbe32915c8325e13e9d812b88137ee4a9c3dbb25))
* fix bug in flavor deserialization ([3e40177](https://github.com/jolars/panache/commit/3e401771ab01825ff088f666b3ce64828a540510))
* fix clippy problems ([5996d90](https://github.com/jolars/panache/commit/5996d90533ab8cad1d4db7f40e7cb32f5c6d5a8f))
* fix erroneous handling of blanklines in indented code ([d058b61](https://github.com/jolars/panache/commit/d058b61572a359774fdda3f4604e1939378d2f49))
* fix some linting issues ([11fc9a7](https://github.com/jolars/panache/commit/11fc9a758c78c0f719c6ebd08334fe616150e9e9))
* handle code blocks nested in lists ([761737d](https://github.com/jolars/panache/commit/761737dbc119b98aaf4f2fae74c9599e1fea3f78))
* **lsp:** correctly compute range to replace ([056f5cc](https://github.com/jolars/panache/commit/056f5cca2475a1a37d5d733c5f25b6e6fcdb7a49))
* properly emit table blanklines into AST ([c48fc9e](https://github.com/jolars/panache/commit/c48fc9e9b99a3971cf390472bfa3beb7ff2d2fe3))
* properly handle code blocks in lists ([42930e0](https://github.com/jolars/panache/commit/42930e0f2947e7c90590da9bb9d38d33faa81b51))
* refactor parser to capture lossless tree ([9bbfd9f](https://github.com/jolars/panache/commit/9bbfd9f35c1ed8e5dd892cd9bce3a5541993fb96))
* use async formatter in LSP formatting ([8efbb1a](https://github.com/jolars/panache/commit/8efbb1ac465fddb3bdbd731e23a4e3febc8d4c07))

## 1.0.0 (2026-02-11)

### ⚠ BREAKING CHANGES

* force subcommand use, add config to parse
* use block parser in formatter
* rename WrapMode options
* change second argument in `format()` to `Config`

### Features

* add `blank_lines` option ([c1080a4](https://github.com/jolars/panache/commit/c1080a42da9bbb6bc4c44c2a5dbad03d719c52ca))
* add `CodeSpan` to syntax ([4e63609](https://github.com/jolars/panache/commit/4e63609709c55e3e63cf8bb110f106f5c2422282))
* add `parse()` function ([18b85ac](https://github.com/jolars/panache/commit/18b85acbe9742f4eba22b2173b654ad6394768f3))
* add a block parser ([200965d](https://github.com/jolars/panache/commit/200965d5b328755afbc6d25ba43b0f228b9c49a2))
* add a LSP ([5befe3d](https://github.com/jolars/panache/commit/5befe3d221fa8fc15e89e82d28ddc613a380ac8b))
* add automatic flavor detection and configuration settings ([bf96aee](https://github.com/jolars/panache/commit/bf96aee2e7450f96d540695145c2502ff7524dd9))
* add basic formatter ([de69b6c](https://github.com/jolars/panache/commit/de69b6ca1b2221de514168d1b61b3e851624e967))
* add blank line after headings ([ee6f3e9](https://github.com/jolars/panache/commit/ee6f3e93c25bb889562706722184d5f57e517298))
* add completion ([7b74ed3](https://github.com/jolars/panache/commit/7b74ed3fc5effb62b4f8bb5f0a2422b9d8fcf95e))
* add emphasis ([c348dd2](https://github.com/jolars/panache/commit/c348dd2b10bf0a4b9164e0cac47afefa09975cad))
* add formatter playground ([2cd7148](https://github.com/jolars/panache/commit/2cd71484180db8c2634357242a52dcfde2f20f46))
* add line ending normalization and detection ([2e06143](https://github.com/jolars/panache/commit/2e0614363a7c307967bc97cfa094afebe2aa9e25))
* add parse subcommand ([f220fb3](https://github.com/jolars/panache/commit/f220fb37a4623b5af46c95a78c83200988454254))
* add placeholder for inline parser ([891883d](https://github.com/jolars/panache/commit/891883d9a150f34557da7c7737ef70d33a030cec))
* add support for footnote references ([cdbd4f8](https://github.com/jolars/panache/commit/cdbd4f82410b3721c6e6e54ca654b39d4e185fd5))
* add support for link attributes ([8ee3d98](https://github.com/jolars/panache/commit/8ee3d98f8dfc0ed72beb77f41297105f1a3b7629))
* add support for using remporary files with extformat ([b7f68a1](https://github.com/jolars/panache/commit/b7f68a14a04f1459416be33a5dafc6547085fc1f))
* break math blocks onto separate lines ([7727bba](https://github.com/jolars/panache/commit/7727bba09762a3d43660b6ba41e39569ef3eb72f))
* change second argument in `format()` to `Config` ([3f993e8](https://github.com/jolars/panache/commit/3f993e86afe41007d79f4d348628d0de8ace0a9a))
* corectly parse inline math ([085081c](https://github.com/jolars/panache/commit/085081cd9d799b7a9427b1f462f6b3398ec1626b))
* create custom paragraph wrapper ([15a1203](https://github.com/jolars/panache/commit/15a1203dcebc2d1f3fcb310d7e005f2ff3e6224c))
* enable bracketed spans and native spans by default ([788009c](https://github.com/jolars/panache/commit/788009ce20f892f4e46b3442f8a8849ae966addd))
* enable configurable backlash math support ([a207b1f](https://github.com/jolars/panache/commit/a207b1ffc1005d43f9af75e2def9447116f5faff))
* force subcommand use, add config to parse ([0fe779f](https://github.com/jolars/panache/commit/0fe779fb17681ebc2f3f2b794ba5c8d65faced00))
* handle headerless simple tables ([e346cf1](https://github.com/jolars/panache/commit/e346cf14b35a29239eb1481b70b8ebcfc4de4d9c))
* handle labels after equations ([826b61b](https://github.com/jolars/panache/commit/826b61b8e9657387b5570bbbd17506135bc67d04))
* implement backslash escape sequences ([8140e7f](https://github.com/jolars/panache/commit/8140e7f815ef3a1a301f0cd477f1636a0da0e055))
* implement code fences in block parser ([0c04bce](https://github.com/jolars/panache/commit/0c04bce9b1b76bca1cf30ca1a60713caf39088fc))
* implement config system for extensions ([8b3c02b](https://github.com/jolars/panache/commit/8b3c02b743ae7a80a00b74168d11f4c663d5c196))
* implement inline code span parsing ([00ed086](https://github.com/jolars/panache/commit/00ed086069717761d21f660d0be4e34f95a4e1a4))
* implement inline math parsing ([3fa4ca0](https://github.com/jolars/panache/commit/3fa4ca037864528549a47583fe0d2bbae5764838))
* implement line blocks ([56e285d](https://github.com/jolars/panache/commit/56e285d2a2502ead1e818442947fa4f88aed9415))
* improve handling of frontmatter in lexer and parser ([a4f0821](https://github.com/jolars/panache/commit/a4f0821c88b6618cd35648474cb9bc8ca6cfacf0))
* make block parser recursive ([60b0438](https://github.com/jolars/panache/commit/60b0438b009b01fc83df4a61eecb1328ef3235a2))
* normalize emphasis ([6ba2061](https://github.com/jolars/panache/commit/6ba2061736d4f623cb865f1bd583936b47dec764))
* package as flake ([b24730b](https://github.com/jolars/panache/commit/b24730bbbed5c11945b23f90a36a75185c761c5e))
* parse `BlankLine` in lexer ([d727494](https://github.com/jolars/panache/commit/d7274942a4878cdeb7d6510e2612fbec9d70f316))
* parse and format headings ([cc4f95c](https://github.com/jolars/panache/commit/cc4f95cde3c3cdf5985aee7d3a494747c506dbb9))
* parse div blocks ([df2e717](https://github.com/jolars/panache/commit/df2e71772c1ba813a04ae351d7c31c1a5ca8e290))
* parse horizontal rules ([9b48280](https://github.com/jolars/panache/commit/9b482807d23047b2227c1e701493b02afb492cbd))
* parse inline math as part of syntax ([d8ce545](https://github.com/jolars/panache/commit/d8ce54502b8cb745fbaa14ae5b393915fab2d6ca))
* partially implement reference links ([93fa82d](https://github.com/jolars/panache/commit/93fa82dbdde3efe95c98d0b70b73456022b8171d))
* properly format code blocks ([9e8e256](https://github.com/jolars/panache/commit/9e8e256f6679c0efb9b6d6be9b2d100fefc9f906))
* rename package to panache ([e64efb4](https://github.com/jolars/panache/commit/e64efb422a408ca2b4b2b448ae9ca5f0e25e3061))
* rename WrapMode options ([f6a6b55](https://github.com/jolars/panache/commit/f6a6b555a5be19b90e943f2c551391f80c647e38))
* show nice diffs with `--check` argument ([807428c](https://github.com/jolars/panache/commit/807428ccf71f0e434b8bf3d6671aa0a266e78eb6))
* suppor bracketed spans ([55668d3](https://github.com/jolars/panache/commit/55668d34ac31dce1871795b82e6bad8d32d15ed0))
* support citations ([4d30e28](https://github.com/jolars/panache/commit/4d30e285994c4151b3c94f217e8ff3145ac1e4e5))
* support definition lists ([3c64756](https://github.com/jolars/panache/commit/3c647566f78127ac9e104c9eb6d177798aea9016))
* support display math ([88a2d4a](https://github.com/jolars/panache/commit/88a2d4ace234b3cd05523201214a96c44616fe17))
* support example lists ([84a5ed6](https://github.com/jolars/panache/commit/84a5ed606f0ae0172e688aed9dd3f10340bdead3))
* support external formatting ([10aed07](https://github.com/jolars/panache/commit/10aed0706875ff541412957f5a8afa18d4c47b6a))
* support fancy lists ([4b41828](https://github.com/jolars/panache/commit/4b418280b7daf5b313b5dd82452e6280a7837e3c))
* support fenced divs ([cf2bafa](https://github.com/jolars/panache/commit/cf2bafadb6918c4bc9775b83a1a399f8823e9962))
* support formatting for pipe tables ([ce4378f](https://github.com/jolars/panache/commit/ce4378f07561ac5a7374e523cbf762e7f2864809))
* support grid tables ([642a8a3](https://github.com/jolars/panache/commit/642a8a338b513f12291efa00cb05123782e87e7a))
* support header attributes ([daa3fca](https://github.com/jolars/panache/commit/daa3fca3964c079bd00cfc36d42cc96199bc0e4b))
* support horizontal rules ([362357a](https://github.com/jolars/panache/commit/362357a8c0156703271b834a74ab98fce6556ec9))
* support image attributes ([f67f682](https://github.com/jolars/panache/commit/f67f682bc4b99a61f1a55a4ec7f88b911e6b4182))
* support images ([3b76a50](https://github.com/jolars/panache/commit/3b76a50c679929f06523d890aded759a6bcc8b27))
* support indented code blocks ([097239b](https://github.com/jolars/panache/commit/097239b1d0a109519cb4c666c0450bf0eede1876))
* support inline code attributes ([0feac47](https://github.com/jolars/panache/commit/0feac472a2be2d7c06fc89bfa7fce95da1e9c356))
* support inline footnotes ([c54bd3b](https://github.com/jolars/panache/commit/c54bd3b22d8ccf8f6f84817593dec7ea5479f4e8))
* support inline footnotes ([e379f65](https://github.com/jolars/panache/commit/e379f65a070e4fead28f958a0116908e947f6ec9))
* support inline latex ([81d7ee0](https://github.com/jolars/panache/commit/81d7ee0051f9a2066620a0ced9d05d5244aeb8a5))
* support inline links ([9d052dd](https://github.com/jolars/panache/commit/9d052ddfb1a697d57ae1f649a9ce3bce0902f869))
* support inline raw attributes ([189ded7](https://github.com/jolars/panache/commit/189ded7a0663073de473624771ff4f9e1ac97257))
* support latex blocks ([e211119](https://github.com/jolars/panache/commit/e2111196c53d3e92c10d841746aa59d0f6651905))
* support lazy block quotes ([6fa9e53](https://github.com/jolars/panache/commit/6fa9e53dfe8ac46f7fe372bfe0a7421c1ad91fd7))
* support lists ([e650b12](https://github.com/jolars/panache/commit/e650b125c9868c8ff60c602c4dfc0973ec1ecf2e))
* support metadata blocks ([7e4d320](https://github.com/jolars/panache/commit/7e4d3207f889e958b6b784507da57175359b51e6))
* support multiline tables ([0ecdf67](https://github.com/jolars/panache/commit/0ecdf67e190de323a4880f38d4ed630caf98e1e3))
* support native spans ([f57bdf2](https://github.com/jolars/panache/commit/f57bdf22f621f090bdf8b307f6676ff3176eb9a1))
* support pipe tables ([a9730cc](https://github.com/jolars/panache/commit/a9730ccf031aee64770529857a46c204426f1bf7))
* support raw blocks ([c17761e](https://github.com/jolars/panache/commit/c17761e30bf29a0851c347ae5335da31f26aa4d8))
* support raw html ([1839481](https://github.com/jolars/panache/commit/1839481640cddfbba6afc749da71b5a50cac2f94))
* support reference images and links ([0a5389d](https://github.com/jolars/panache/commit/0a5389d5aed9ea10d9f74de0fc9242154c9c7b01))
* support simple tables ([7f808ca](https://github.com/jolars/panache/commit/7f808ca723fca78bcb68ec3ae10100ceeb7720ba))
* support simple tables ([dba5cbf](https://github.com/jolars/panache/commit/dba5cbf17953e45aa5b3865031c28b5562c76999))
* support single and double backslash math ([9a72c6a](https://github.com/jolars/panache/commit/9a72c6a996b624b99d7008c31855f5f3b515bb14))
* support strikethrough ([5e4cb3b](https://github.com/jolars/panache/commit/5e4cb3bd0f3b21bab38a18e68b394ba141ea56e9))
* support sub- and superscript ([e313a81](https://github.com/jolars/panache/commit/e313a811750a9fbf93f5b59c02ab286b1ed03002))
* support table captions ([22240c5](https://github.com/jolars/panache/commit/22240c53280357e552b87e28b4474989de3d2055))
* use `rmarkdown` not `r-markdown` ([235363f](https://github.com/jolars/panache/commit/235363fe08a6736c1bd7be39c5b35554e22ac26d))
* use block parser in formatter ([60cb5b4](https://github.com/jolars/panache/commit/60cb5b4a856faf1626277a81ae2bedb6d29af263))

### Bug Fixes

* add basic handling of comments ([578f72f](https://github.com/jolars/panache/commit/578f72f41a28b4c24d0454eb55e50c525e710527))
* add missing stdin field ([4e27a82](https://github.com/jolars/panache/commit/4e27a8261487dc3b24f5e5cb8c68e01ac8cfaed8))
* add support for tex commands ([21c2f9b](https://github.com/jolars/panache/commit/21c2f9b96c640db8e81ea553f02775a116425453))
* allow multiple frontmatter blocks ([6e81a0d](https://github.com/jolars/panache/commit/6e81a0d6f9a92163c2a6b5ab64c5329390f248b2))
* **config:** avoid panic when unwrapping non-existent config ([752a72f](https://github.com/jolars/panache/commit/752a72fc7f8670da66a9c4fd6cae7a1267949ad4))
* correctly align and format right-aligned lists ([d15e8d8](https://github.com/jolars/panache/commit/d15e8d851c431a8e7e183bcdb07ad73b58802a4b))
* correctly catch horizontal rule with `*` ([7ae1e37](https://github.com/jolars/panache/commit/7ae1e379db8ffa70e0176d3f2a093c463d355f59))
* correctly extract language from blocks ([548d7c3](https://github.com/jolars/panache/commit/548d7c3f04c845a0a097ab6691017345f25af92d))
* correctly handle lazy continuation in definition lists ([47cbcc6](https://github.com/jolars/panache/commit/47cbcc6d3e65f1319ff14b6e50bd39e98170bf70))
* correctly parse bracketed spans in headings ([772656e](https://github.com/jolars/panache/commit/772656e2918a55e46087a4e979937554cbe27700))
* correctly parse commend end ([88a612c](https://github.com/jolars/panache/commit/88a612c00802a7659afa7f154759d3b61e0d0728))
* correctly parse headerless simple tables ([325b2c4](https://github.com/jolars/panache/commit/325b2c4ead54e398983a0b29a5a346c5a3430028))
* correctly parse html comments without preceding space ([e7180fd](https://github.com/jolars/panache/commit/e7180fd814408749edeca1ac2ba5fa0ae86ddcc3))
* correctly parse hyphens in text as non-list markers ([3eaa872](https://github.com/jolars/panache/commit/3eaa872fc370114212b89331c7b9d63d43891642))
* correctly sparse task list checkboxes ([037db65](https://github.com/jolars/panache/commit/037db6547ae2cfdc5466f14480d375625f36e245))
* correctly wrap flat lists ([afed9e3](https://github.com/jolars/panache/commit/afed9e3c73fcd2699ec1be173efc332b0a7f0aa7))
* correctly wrap in lists ([c06a73c](https://github.com/jolars/panache/commit/c06a73cf2d3d8f6d31b7df22dbe8e1f6b0c40e83))
* correctly wrap list items ([038b57a](https://github.com/jolars/panache/commit/038b57a80b827a772e20c2da62b1cd6e09434968))
* don't wrap math ([4e876c1](https://github.com/jolars/panache/commit/4e876c1ebb8ca2d8e2374c10904905f89a0f16ca))
* enforce Pandoc spec rules for inline math parsing ([2612ae5](https://github.com/jolars/panache/commit/2612ae5749aaa0412891845099d36fd7e1532818))
* fix clippy lints ([a5c646f](https://github.com/jolars/panache/commit/a5c646f6bf090a10371fccffddc2084277a3d8bd))
* fix continuation bug ([9e24a23](https://github.com/jolars/panache/commit/9e24a23249bacebe1fef78482baf3e1cc5a36898))
* fix failing test due to formatting ([96b4ec4](https://github.com/jolars/panache/commit/96b4ec409f7680fe66024a51845a7e05c0b1147b))
* fix handling of block quotes ([7a421af](https://github.com/jolars/panache/commit/7a421afcafeee6b9b686ba4e5c13c9b691387bcb))
* fix handling of fenced code blocks ([7a45752](https://github.com/jolars/panache/commit/7a45752d7044a49fa6ea7034056ebd6ca6ba983f))
* fix infinite loops ([62365e9](https://github.com/jolars/panache/commit/62365e95eff5bbf1c280e5ee8408c994789a5cf4))
* fix lint errors ([1326251](https://github.com/jolars/panache/commit/1326251524bdde3c42fa11f0ce1b65d57f2af3c8))
* fix linter warning ([9ad69a9](https://github.com/jolars/panache/commit/9ad69a9fb5f5b4d5f17f77b0c54047501ed58265))
* fix list indentation issue ([674c0b0](https://github.com/jolars/panache/commit/674c0b07374e2f338296c97a78ba0b32987f4c18))
* fix missing quote markers ([0685219](https://github.com/jolars/panache/commit/0685219ce8319a318387d7e6dc0f6a0276a2c34d))
* fix pandoc defaults ([62f6eb7](https://github.com/jolars/panache/commit/62f6eb740dc30ad13e71fc0a876b361796cc6f98))
* fix some clippy issues ([c36caa7](https://github.com/jolars/panache/commit/c36caa712baa5073e65a3330ad065000b6c098e2))
* fix word wrapping ([5cf939d](https://github.com/jolars/panache/commit/5cf939d415380bcced9209c35e652e1318164a6b))
* format syntax ([f00cc8a](https://github.com/jolars/panache/commit/f00cc8af73c7a390d48a811570437b0ca43b614c))
* handle headerless simple tables ([202858d](https://github.com/jolars/panache/commit/202858dfb3ea0106a83943d60cbb283136282765))
* handle lazy block quotes ([d92a732](https://github.com/jolars/panache/commit/d92a73275677a025be3826262c8bb77dce842f2b))
* handle links and images as children of a paragraph ([5f13634](https://github.com/jolars/panache/commit/5f13634e4f42d920d894ab293e0289ba10b449a0))
* handle links properly ([50b8475](https://github.com/jolars/panache/commit/50b847590fbbc9a73ce53a903ad7c0a8e29e91c6))
* handle nested block quotes ([7b92701](https://github.com/jolars/panache/commit/7b927019141cf7e8d1f2d6f69820616adde3102d))
* handle nested lists ([198a811](https://github.com/jolars/panache/commit/198a81144a2719a8fc8a3be4c007bbf8e4f898d3))
* handle tex environments ([f952861](https://github.com/jolars/panache/commit/f95286112c62f38fa66a83f4bb0d510e1144429e))
* handle wrapping around punctuation correctly ([ed79abc](https://github.com/jolars/panache/commit/ed79abcd5453929335e84331f8810afe67eb7bd4))
* improve list continuation parsing ([2f5bc99](https://github.com/jolars/panache/commit/2f5bc9927eb72c8e9daeea061a7ffb06c49228f2))
* initalize logger conditionally inside `format()` ([15b9be3](https://github.com/jolars/panache/commit/15b9be3fea2d577e6c7919ba90373df0e4007470))
* **lexer:** correctly parse `$$$` as block math ([59446d7](https://github.com/jolars/panache/commit/59446d7d7b8aa3dcbe095d5b51baa98061ac2f4a))
* make block quote parsing more robust ([c361bcf](https://github.com/jolars/panache/commit/c361bcfeb882bac5524dd7d6ad2f95e0b66cf282))
* normalize line endings to unix style ([88c000f](https://github.com/jolars/panache/commit/88c000f8b239327fb9158363ffcd6ff9d9b0da2e))
* omit block quote markers from wrapped paragraph ([d067268](https://github.com/jolars/panache/commit/d06726818e180109d799bfaf98c46af0238f8ae8))
* pandoc has raw_tex by default ([3e83ccb](https://github.com/jolars/panache/commit/3e83ccb6a61ef74de3f6ab4d745f8e64d01dec04))
* parse dollar signs as text ([2503bed](https://github.com/jolars/panache/commit/2503bed1b7780e7b2e9c12334772965563793553))
* parse inline math as part of paragraph ([2e42843](https://github.com/jolars/panache/commit/2e42843c17c4cc8ce2d5c6f4dfb7e6dffce5b3fb))
* properly handle attributes ([75f5d43](https://github.com/jolars/panache/commit/75f5d43631666fce1a074f30174fd8ada6f9222e))
* properly handle fenced divs ([eed54f8](https://github.com/jolars/panache/commit/eed54f826fba97e1f0231085beb7baa6675050f3))
* properly handle lazy continuation ([5bd232b](https://github.com/jolars/panache/commit/5bd232bb7eb903be3f59ce20a17ac9470e843204))
* remove clippy warnings ([d8819c3](https://github.com/jolars/panache/commit/d8819c37832b2cf7f0d2b393a109ff0f5bf0fa1c))
* remove unchanged variable ([199b77a](https://github.com/jolars/panache/commit/199b77af6608a5842083613dac2f52191b9bd763))
* support numbered lists ([5435b5f](https://github.com/jolars/panache/commit/5435b5f8a97760bb94bf9b74cf90a4def6951d42))
* use a for loop instead of while ([8c99913](https://github.com/jolars/panache/commit/8c99913eea450f240a50936310a8c6258f102e9c))

### Performance Improvements

* add `byte_offset` to avoid recomputing each time ([bc97d3b](https://github.com/jolars/panache/commit/bc97d3bea6749aa3fc677652c4115bc9b9663bea))
* disable `debug!` and `trace!` in release builds ([b40d27c](https://github.com/jolars/panache/commit/b40d27cc7729540d7acb22473bef22f5ad5aef77))
* move assertion into debug profile ([fa73acd](https://github.com/jolars/panache/commit/fa73acd830116a07f42e78c0f9edddf4d136c33b))
* preallocate string size ([d989e26](https://github.com/jolars/panache/commit/d989e26ac640b5eb27d588d524d0e04671c7e202))
* reduce allocations in wrap_text ([3ccda66](https://github.com/jolars/panache/commit/3ccda66a6b4b66c003f4e1a36bb8330f273ddee7))
* simplify paragraph wrapper ([8235242](https://github.com/jolars/panache/commit/8235242d6ae741c09513d5f90a0fd8b3b92f1720))
* switch to trace logging ([e4a0beb](https://github.com/jolars/panache/commit/e4a0bebf17a19de96f994a0ab7eefc2c367ba4a1))
