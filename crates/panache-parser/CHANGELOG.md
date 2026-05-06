# Changelog

## [0.7.1](https://github.com/jolars/panache/compare/panache-parser-v0.7.0...panache-parser-v0.7.1) (2026-05-06)

### Bug Fixes
- enable `autolinks` for GFM ([`aeda13c`](https://github.com/jolars/panache/commit/aeda13cdc71a002bf0326cab9c1354abec321b2a)), closes [#258](https://github.com/jolars/panache/issues/258)

## [0.7.0](https://github.com/jolars/panache/compare/panache-parser-v0.6.1...panache-parser-v0.7.0) (2026-05-05)

### Features
- **linter:** add linting rule for bad HTML entities ([`93aa280`](https://github.com/jolars/panache/commit/93aa2804dcd6d874d2c02b149ecead83233d9bc0)), closes [#251](https://github.com/jolars/panache/issues/251)
- wire new reference impl into salsa and CST ([`3ba22c1`](https://github.com/jolars/panache/commit/3ba22c1700591cd6d1c173d74416c97987a33fa0))
- add `parse_with_refdefs` and `UNRESOLVED_REFERENCE` ([`e6c17fb`](https://github.com/jolars/panache/commit/e6c17fb6f2903c74bbe547b19200abcb381dcc4d))
- **parser:** expose pandoc-native projector as public API ([`5b79b92`](https://github.com/jolars/panache/commit/5b79b92647fe889fcd1179e1145902bb4588f22e))

### Bug Fixes
- **parser:** degrade unresolved bracket if inner emph leaks ([`e1c291b`](https://github.com/jolars/panache/commit/e1c291b0b2f478324e91e90e4895333d099c89e9)), closes [#250](https://github.com/jolars/panache/issues/250)
- handle ambiguous markers and indented code block ([`8d3db6d`](https://github.com/jolars/panache/commit/8d3db6d5937137ae825523f0f8141edcdd200fa4))
- **parser:** allow drift tolerance for list parsing ([`1836a7b`](https://github.com/jolars/panache/commit/1836a7b748c127ffe794a137df91940f30567382)), closes [#246](https://github.com/jolars/panache/issues/246)
- **parser:** handle tilde-fences dispatch correctly ([`519abd1`](https://github.com/jolars/panache/commit/519abd1c12dff37331e9aad3d2baefe4b7701fb9)), closes [#248](https://github.com/jolars/panache/issues/248)
- **parser:** fix byte-order breakage in tilde-fenced code ([`18ca6c2`](https://github.com/jolars/panache/commit/18ca6c2bec5e46ee241df774e772f2e37105ed5a)), closes [#249](https://github.com/jolars/panache/issues/249)
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

## [0.6.1](https://github.com/jolars/panache/compare/panache-parser-v0.6.0...panache-parser-v0.6.1) (2026-05-01)

### Bug Fixes
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
- **parser:** add IR-driven dispatch for Pandoc links/images ([`1e4227e`](https://github.com/jolars/panache/commit/1e4227e94e1c110f99a4e5185f3b13cdc58825d5))
- **parser:** add IR-driven dispatch for [text]{attrs} ([`cf50ec5`](https://github.com/jolars/panache/commit/cf50ec5c7d5572bad8a6b5989c34e7b0c593a12a))
- **parser:** add IR-driven dispatch for citations ([`9e826db`](https://github.com/jolars/panache/commit/9e826db3c488fecb821f42a22410a34297690b18))
- **parser:** add IR-driven dispatch for [^id] footnote refs ([`614221e`](https://github.com/jolars/panache/commit/614221e5b9d0d2819b50abdd6d499fd87509c8c2))
- **parser:** add IR-driven dispatch for ^[note] and <span> ([`1b9e618`](https://github.com/jolars/panache/commit/1b9e61876896c36964dba36ffdc60bcf489c7309))

## [0.6.0](https://github.com/jolars/panache/compare/panache-parser-v0.5.1...panache-parser-v0.6.0) (2026-04-29)

### Features
- **parser:** handle inline HTML ([`5fb7272`](https://github.com/jolars/panache/commit/5fb727257c0b2d6385b22e29a64f2bde1d0196f4))
- add `Dialect` to untangle CommonMark from Pandoc ([`a1cb7df`](https://github.com/jolars/panache/commit/a1cb7df9ca8461f45db2b7f4efb50e57e8febce3))

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
- **parser:** don't emit synthethic token ([`a137fc4`](https://github.com/jolars/panache/commit/a137fc4d6352890a44ff47c247072be90077e8a0)), closes [#235](https://github.com/jolars/panache/issues/235)
- **parser:** handle autolinks and blockquotes for cmark ([`b1cedd4`](https://github.com/jolars/panache/commit/b1cedd4f586ea53b7174a039d37f2160c1dcdfab))
- **parser:** handle HTML blocks for pandoc/commonmark ([`227648e`](https://github.com/jolars/panache/commit/227648e07760c65282372dab159ca50bb5e32f09))
- **parser:** handle pandoc/cmark difference in fenced code ([`b370edd`](https://github.com/jolars/panache/commit/b370eddfd66d67b4e4865b177729a78af5b27af2))
- **parser:** handle backslash escapes, autolinks, empty code ([`317b150`](https://github.com/jolars/panache/commit/317b150a07783e6b58c8f5de770c2da354af165b))
- **parser:** allow space after atx and any length setext ([`647d274`](https://github.com/jolars/panache/commit/647d2741bc95fcc901b831f26b2de3135b70d4f0))
- **parser:** enable `all_symbols_escapable` for commonmark ([`04c52d7`](https://github.com/jolars/panache/commit/04c52d7a20e0047c618a69f5b38e46f0f379df45))
- handle thematic breaks in commonmark correctly ([`f98fca0`](https://github.com/jolars/panache/commit/f98fca002c517d06a67c443d4c1e841ebe087842))
- **parser:** fix image link handling in commonmark ([`cac6004`](https://github.com/jolars/panache/commit/cac600484142950a97f77a3f3cf0cb8a67e2f21d))
- **parser:** preserve entity references in cmark ([`0ae7579`](https://github.com/jolars/panache/commit/0ae75793f54e59402a4d69f601b449ef681b7e25))
- **parser:** handle ATX headings in commonmark correctly ([`8c09c19`](https://github.com/jolars/panache/commit/8c09c19565292b363fafb1a08fd85a42c721d10d))
- **parser:** add extensions to commonmark flavor ([`59166ab`](https://github.com/jolars/panache/commit/59166ab00fc960b19a259ad31397eb50d541f69c))

## [0.5.1](https://github.com/jolars/panache/compare/panache-parser-v0.5.0...panache-parser-v0.5.1) (2026-04-27)

### Bug Fixes
- **parser:** include `~` in set of escapables ([`cfc0bfc`](https://github.com/jolars/panache/commit/cfc0bfcd5cf1e02fd7ef16b712d666df61e260b6)), closes [#231](https://github.com/jolars/panache/issues/231)
- **parser:** handle consecutive footnote definitions ([`e694627`](https://github.com/jolars/panache/commit/e694627654c497b66328d6062aa392af7337ce34))

## [0.5.0](https://github.com/jolars/panache/compare/panache-parser-v0.4.2...panache-parser-v0.5.0) (2026-04-27)

### Features
- **cli:** make `--debug` actually useful in release builds ([`92a54ec`](https://github.com/jolars/panache/commit/92a54ecc087a10347a94fccfb7210dfdc345220f))

### Bug Fixes
- **parser:** emit empty cells for degenerate cells ([`095ada7`](https://github.com/jolars/panache/commit/095ada7da13f020de9856ae0ac06d2d441d451cd)), fixes [#224](https://github.com/jolars/panache/issues/224)

## [0.4.2](https://github.com/jolars/panache/compare/panache-parser-v0.4.1...panache-parser-v0.4.2) (2026-04-24)

### Bug Fixes
- **formatter:** don't break display math inside emphasis ([`d2eee34`](https://github.com/jolars/panache/commit/d2eee343d1e5099ca28a7a7dec50fb4aa9ca5f0b)), closes [#214](https://github.com/jolars/panache/issues/214)
- handle UTF-8 boundary bug in table parsing ([`2c4e20f`](https://github.com/jolars/panache/commit/2c4e20f1039f97468879d083d87a878a09f79d96)), closes [#211](https://github.com/jolars/panache/issues/211)
- **parser:** don't let definition list adopt trailing list ([`b2fba48`](https://github.com/jolars/panache/commit/b2fba48ab289b077a8d98c55152c61be7c978aa1))
- properly parse and format blockquote markers in deflist ([`b27eeb7`](https://github.com/jolars/panache/commit/b27eeb77aaf833aba1ab1370504b90b8a6e2d252)), closes [#209](https://github.com/jolars/panache/issues/209)
- **parser:** correctly emit blanklines in tables/captions ([`0465f45`](https://github.com/jolars/panache/commit/0465f45dc437a7b8e0c751e672bc85e3806320d8)), closes [#210](https://github.com/jolars/panache/issues/210)
- **parser:** allow Rcpp as known language in hahspipe parse ([`0fd5979`](https://github.com/jolars/panache/commit/0fd5979634810bbe2c42c238657b37b161d237a2))

## [0.4.1](https://github.com/jolars/panache/compare/panache-parser-v0.4.0...panache-parser-v0.4.1) (2026-04-22)

### Bug Fixes
- **parser:** don't parse caption as definition ([`e542c1f`](https://github.com/jolars/panache/commit/e542c1f59c3917feb885153590574eb22677818d))
- greedily consume table captions ([`58afc1c`](https://github.com/jolars/panache/commit/58afc1c2c27182a7e9768a1ff3f3b2b6e82531d5))
- **parser:** handle empty lines in hashpipe normalizer ([`51e6146`](https://github.com/jolars/panache/commit/51e614637bcd003f9970a546c540eaa92e0c3ea1)), closes [#201](https://github.com/jolars/panache/issues/201)
- **parser:** don't drop adjacent table caption ([`9144d63`](https://github.com/jolars/panache/commit/9144d636480e422378b929d0e03dd60cd31a719a)), closes [#200](https://github.com/jolars/panache/issues/200)
- **parser:** properly handle adjacent tables ([`6206623`](https://github.com/jolars/panache/commit/6206623319b1a545fceedc67f5f6fa2596d9c1d8))
- **parser:** don't treat `:` table caption as def list ([`a287631`](https://github.com/jolars/panache/commit/a287631f90a0707b337f1d4438bb4bb9f8a28475))
- **parser:** handle bare URI in gfm flavor properly ([`2559a99`](https://github.com/jolars/panache/commit/2559a9958f70b4ba17abedc20a4c20bc85779053)), closes [#197](https://github.com/jolars/panache/issues/197)
- **parser:** correctly parse deep list in blockquote ([`51484ac`](https://github.com/jolars/panache/commit/51484ac9b640278ea9eff860db6857cdcf07a931)), closes [#195](https://github.com/jolars/panache/issues/195)
- avoid wrapping on fancy markers in unsafe contexts ([`4de13dd`](https://github.com/jolars/panache/commit/4de13dd0fe44b9bb728d7aa22b772a2267cf060b)), closes [#193](https://github.com/jolars/panache/issues/193)
- **parser:** handle varying indentation for blockquotes ([`cdd3eec`](https://github.com/jolars/panache/commit/cdd3eec2c4b555476ed96d5c02dfd3a056876e86)), closes [#186](https://github.com/jolars/panache/issues/186)
- **parser:** accept empty headings ([`d081dd7`](https://github.com/jolars/panache/commit/d081dd72b5537b55ccb047879732ebf51df6ee4c))
- **parser:** fix logic around `blank_before_header` ([`c8f48c9`](https://github.com/jolars/panache/commit/c8f48c9ad69d3a3780a1a6ef2b300af203960eed))
- **parser:** handle bare `#|` comments ([`1a7d009`](https://github.com/jolars/panache/commit/1a7d009e08a964b059aae40241f70e28b30c5639)), fixes [#188](https://github.com/jolars/panache/issues/188) and [#190](https://github.com/jolars/panache/issues/190)

## [0.4.0](https://github.com/jolars/panache/compare/panache-parser-v0.3.1...panache-parser-v0.4.0) (2026-04-19)

### Features
- support smart punctuation ([`926a4c8`](https://github.com/jolars/panache/commit/926a4c80ed854f5a0afdfdae4d512adf91840525)), closes [#182](https://github.com/jolars/panache/issues/182)

### Bug Fixes
- **parser:** parse display math over paragraph boundary ([`b5c9be2`](https://github.com/jolars/panache/commit/b5c9be2fc8d685df46bcf7cc81625337df53b029)), closes [#176](https://github.com/jolars/panache/issues/176)
- avoid special normalization of yaml and hashpipe items ([`d8bfb76`](https://github.com/jolars/panache/commit/d8bfb760e457d31bbec3ccebb4fb2089940a9377))
- **parser:** handle utf-8 slicing in inline spans ([`8ccfe5c`](https://github.com/jolars/panache/commit/8ccfe5cee410162c84f85053528b5f829dc85c81)), closes [#175](https://github.com/jolars/panache/issues/175)
- **parser:** flush list-item inline buffer ([`a49179b`](https://github.com/jolars/panache/commit/a49179b14dbb6e753c2a2505a19df8c4e1d80afa)), closes [#174](https://github.com/jolars/panache/issues/174)
- **parser:** enable `inline_link` for GFM flavor ([`8059792`](https://github.com/jolars/panache/commit/805979269e898a4f28faddd15dcd07f2593f37ab)), closes [#171](https://github.com/jolars/panache/issues/171)

## [0.3.0](https://github.com/jolars/panache/compare/panache-parser-v0.2.1...panache-parser-v0.3.0) (2026-04-14)


### Features

* **parser:** add support for `mark` extension ([888c810](https://github.com/jolars/panache/commit/888c8103fa46425909f37bf7e94401135bf29731))

## [0.2.1](https://github.com/jolars/panache/compare/panache-parser-v0.2.0...panache-parser-v0.2.1) (2026-04-14)


### Bug Fixes

* handle alignment drift in roman list labels ([7627267](https://github.com/jolars/panache/commit/7627267bb3d6c3c34602f61ad61eb81de72ec2e4)), closes [#136](https://github.com/jolars/panache/issues/136)
* **parser:** handle deep indentation and roman nos in list ([04b80f5](https://github.com/jolars/panache/commit/04b80f56f09801a9cfa1449c0f5e39670c9b6cfe)), closes [#143](https://github.com/jolars/panache/issues/143)
* **parser:** handle deep roman list and quotation ([b7aac81](https://github.com/jolars/panache/commit/b7aac81dc67bd38a04238d047d2b4c23d1214992)), closes [#137](https://github.com/jolars/panache/issues/137)
* **parser:** treat `$$\begin{..}` correctly ([cee37c5](https://github.com/jolars/panache/commit/cee37c51dc6898b6d2e45a2434f300ae6d6b7250)), closes [#134](https://github.com/jolars/panache/issues/134)
* remove test placeholder ([39fd39f](https://github.com/jolars/panache/commit/39fd39f69f5517d72f05a8cc0238f84e1177b487))

## [0.2.0](https://github.com/jolars/panache/compare/panache-parser-v0.1.0...panache-parser-v0.2.0) (2026-04-13)


### ⚠ BREAKING CHANGES

* use flat `ParserOptions`
* drop use of `Config`

### Features

* drop use of `Config` ([036fca7](https://github.com/jolars/panache/commit/036fca7e722c2d11ad70fbca66e97003b65c46b6))
* use flat `ParserOptions` ([57a7363](https://github.com/jolars/panache/commit/57a736360f1ad2bfba43f3c01cf64a3d1faec774))


### Bug Fixes

* **parser:** fix continuation detection in indented context ([4f1e51d](https://github.com/jolars/panache/commit/4f1e51d7fd0b8cc795747b95f3c223826832c9d7)), closes [#139](https://github.com/jolars/panache/issues/139)
* **parser:** mitigate UTF-8 panic in hashpipe path ([26c702d](https://github.com/jolars/panache/commit/26c702dd0f66f8e3e36a7476e813eea3bc5ab2ee)), closes [#135](https://github.com/jolars/panache/issues/135)


### Reverts

* "chore(release): release 2.33.0 [skip ci]" ([01ac037](https://github.com/jolars/panache/commit/01ac037dc55b39ddcda83f5243e5e3a0192314fd))
