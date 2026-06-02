# Changelog

## [0.9.0](https://github.com/jolars/panache/compare/panache-formatter-v0.8.0...panache-formatter-v0.9.0) (2026-06-02)

### Features
- **config:** abort on unknown extensions, add exts to schema ([`397e1e5`](https://github.com/jolars/panache/commit/397e1e58a83e42a1decfb7692114099702fe681d))
- **cli:** allow `-o extensions.<name>=<bool>` overrides ([`2df73ab`](https://github.com/jolars/panache/commit/2df73ab3153b1f4e009a930536f3f590d1a0ef37))
- **formatter:** add `east_asian_line_breaks` extension ([`4f28716`](https://github.com/jolars/panache/commit/4f2871673d2ba4d00142032d066386db151179e9)), in [#339](https://github.com/jolars/panache/issues/339), closes [#339](https://github.com/jolars/panache/issues/339)

### Bug Fixes
- **formatter:** preserve layout when paragraph swallows a fence shape ([`6458e96`](https://github.com/jolars/panache/commit/6458e96a5e276232866d12225300a61e6e46a8af)), closes [#340](https://github.com/jolars/panache/issues/340)
- **formatter:** keep list marker off reflowed line start ([`68bc1fc`](https://github.com/jolars/panache/commit/68bc1fc8cb43e2e3eea72d7363d8b35c5dad055d))
- **formatter:** keep escaped pipe in table-cell code span ([`0b94ca2`](https://github.com/jolars/panache/commit/0b94ca2537f8b51ddd285468c144c09620b0ecfd))
- **parser:** restrict bare-URI autolinks to known schemes (#337) ([`930db45`](https://github.com/jolars/panache/commit/930db45b8f7bf71f08e3bdb4f036e5a6928936d9)), closes [#336](https://github.com/jolars/panache/issues/336)
- **formatter:** fix panic when formatting `<!--->` ([`b580bb9`](https://github.com/jolars/panache/commit/b580bb9cfa9345787c106a6d3522be2a515fb451))
- **parser:** keep `.class`/`#id` on executable fence info ([`4c8f396`](https://github.com/jolars/panache/commit/4c8f39682b6de5c887f0727a39b0f18b264ec762)), fixes [#334](https://github.com/jolars/panache/issues/334)

### Dependencies
- updated crates/panache-parser to v0.14.0
## [0.8.0](https://github.com/jolars/panache/compare/panache-formatter-v0.7.0...panache-formatter-v0.8.0) (2026-05-29)

### Features
- **formatter:** reflow grid table cells ([`721b110`](https://github.com/jolars/panache/commit/721b1104b609ac9401e0bc8c9faa6dbfb925eaf7)), closes [#323](https://github.com/jolars/panache/issues/323)
- **formatter:** reflow multiline table cells ([`5682db7`](https://github.com/jolars/panache/commit/5682db7e2389f862c90655c55bd2ab1c0cc08248)), ref [#323](https://github.com/jolars/panache/issues/323)

### Bug Fixes
- **parser:** don't swallow space after inline code in emph ([`adf92fa`](https://github.com/jolars/panache/commit/adf92fae91d50c4a9cc82cc10128c8f1232e858b)), closes [#332](https://github.com/jolars/panache/issues/332)
- **formatter:** preserve grid table column widths ([`c4d011b`](https://github.com/jolars/panache/commit/c4d011b4a2b1ca1ab7c2ddc9728f8d3f04724f77))
- keep grid tables at column 0 to match pandoc ([`73016e3`](https://github.com/jolars/panache/commit/73016e3acabdfff0b0c800e8c557ea51a63456b4))

### Dependencies
- updated crates/panache-parser to v0.13.0
## [0.7.0](https://github.com/jolars/panache/compare/panache-formatter-v0.6.1...panache-formatter-v0.7.0) (2026-05-26)

### Features
- **formatter:** add `semantic` wrap mode ([`41f7025`](https://github.com/jolars/panache/commit/41f70254abd7ccbbcfb36cff833c14ed7b81e6f8)), closes [#313](https://github.com/jolars/panache/issues/313)
- **extensions:** support `four-space-rule` extension ([`77768ba`](https://github.com/jolars/panache/commit/77768bab3daec6dbae3a8d1d629add0d4b0700c8)), closes [#308](https://github.com/jolars/panache/issues/308)
- **formatter:** add language-aware and configurable abbrevations ([`ca9b514`](https://github.com/jolars/panache/commit/ca9b5146914cd21141bc6036d48f3e1732085154)), closes [#307](https://github.com/jolars/panache/issues/307)

### Bug Fixes
- **formatter:** keep code spans and autolinks literal under smart ([`7114c5d`](https://github.com/jolars/panache/commit/7114c5d69b600fc39b746b27b606ed838f5110dd))
- **formatter:** normalize smart dashes in headings, guard rule ([`82c9a31`](https://github.com/jolars/panache/commit/82c9a310fc3f88be88b68101e45bcbaa2f7b425c))

### Dependencies
- updated crates/panache-parser to v0.12.0
## [0.6.1](https://github.com/jolars/panache/compare/panache-formatter-v0.6.0...panache-formatter-v0.6.1) (2026-05-20)

### Bug Fixes
- **parser:** strip list+bq prefix on line-block lookahead ([`280c6c1`](https://github.com/jolars/panache/commit/280c6c1774ab2b226c0018fcdc96bb03b4449643))

### Dependencies
- updated crates/panache-parser to v0.11.0
## [0.6.0](https://github.com/jolars/panache/compare/panache-formatter-v0.5.1...panache-formatter-v0.6.0) (2026-05-17)

### Features
- **formatter:** trim trailing blanklines in fenced divs ([`6d2fe6c`](https://github.com/jolars/panache/commit/6d2fe6c55643fcffac29cfa3cda7b96198b71a7b))
- **formatter:** add `""` as configurable external formatter ([`31c0bcb`](https://github.com/jolars/panache/commit/31c0bcb7c1b8d3434bcef78444a6a6ec356c79ad)), closes [#287](https://github.com/jolars/panache/issues/287)

### Bug Fixes
- **formatter:** reflow `BRACKETED_SPAN` content ([`0aac341`](https://github.com/jolars/panache/commit/0aac3414f34136b92b834c55a01effca9a0f0784)), closes [#291](https://github.com/jolars/panache/issues/291)
- **formatter:** collapse blank lines inside fenced divs ([`eb52b1e`](https://github.com/jolars/panache/commit/eb52b1ead93b6bf24a4b44f12a055f09a4d0ba56)), fixes [#286](https://github.com/jolars/panache/issues/286)
- **parser:** lift list-item Comment/PI trailing-text split ([`50b4b45`](https://github.com/jolars/panache/commit/50b4b45db76bbab613322fb8fb71e8ae3ceefa66))
- **parser:** lift same-line HTML block as sole list-item content ([`cb0a2c1`](https://github.com/jolars/panache/commit/cb0a2c1bc707b49a837ce20202eb6b4b59b6b76f))

### Dependencies
- updated crates/panache-parser to v0.10.0
## [0.5.1](https://github.com/jolars/panache/compare/panache-formatter-v0.5.0...panache-formatter-v0.5.1) (2026-05-12)

### Bug Fixes
- **formatter:** don't strip `!expr` in hashpipe yaml ([`f03ca70`](https://github.com/jolars/panache/commit/f03ca702815cbafb54c0066b685ec6497ca968e4)), closes [#280](https://github.com/jolars/panache/issues/280)
- **formatter:** don't skip `PLAIN` in second pass ([`a693f40`](https://github.com/jolars/panache/commit/a693f40488b6fa53726e70260cb66dce2853b5f9)), closes [#279](https://github.com/jolars/panache/issues/279)
- **parser,formatter:** don't escape `[`, `]` ([`26bbb1c`](https://github.com/jolars/panache/commit/26bbb1c5bd539c85108f63e79dbe7c29d24b5222))

### Dependencies
- updated crates/panache-parser to v0.9.0
## [0.5.0](https://github.com/jolars/panache/compare/panache-formatter-v0.4.3...panache-formatter-v0.5.0) (2026-05-09)

### Features
- **parser:** parser inline spans granularly ([`03333d2`](https://github.com/jolars/panache/commit/03333d241000a0cbea6648967bf08fd940b4e0ab))

### Bug Fixes
- **parser,linter:** introduce `HTML_DIV_BLOCK` parsing ([`3962e03`](https://github.com/jolars/panache/commit/3962e0329a83feb5bfbdef84fd3bf52527e7af58)), closes [#263](https://github.com/jolars/panache/issues/263)

### Dependencies
- updated crates/panache-parser to v0.8.0
## [0.4.3](https://github.com/jolars/panache/compare/panache-formatter-v0.4.2...panache-formatter-v0.4.3) (2026-05-06)

### Dependencies
- updated crates/panache-parser to v0.7.1

## [0.4.2](https://github.com/jolars/panache/compare/panache-formatter-v0.4.1...panache-formatter-v0.4.2) (2026-05-05)

### Bug Fixes
- **formatter:** handle nexted list with same line marker ([`8d0653a`](https://github.com/jolars/panache/commit/8d0653a69c1dda3b3a0f07a813c7a44e4efe3766)), closes [#247](https://github.com/jolars/panache/issues/247)
- recursive into linst/blockquote/list ([`175d78e`](https://github.com/jolars/panache/commit/175d78e6ce5287578fe7c7ee5c3c079e674f2663))
- handle pandoc-commonmark divergence on html comments ([`ca301f9`](https://github.com/jolars/panache/commit/ca301f99a4dc74d7d40ad087d59f97928cff5fc4))
- handle same-line block quote marker ([`3c6c3dd`](https://github.com/jolars/panache/commit/3c6c3dd7739ed592d3f6e6c7305a9d616a953fb2))
- **parser:** handle direct list-in-lis correctly ([`5c6a4ae`](https://github.com/jolars/panache/commit/5c6a4ae6ac476232ef6040df586610cfc13f44ef))
- correctly handle definition inside footnote ([`3a30b05`](https://github.com/jolars/panache/commit/3a30b0588acb6a023389fc04604b0ff01d3d6ce4))
- parse and format headings inside lists ([`d7e714e`](https://github.com/jolars/panache/commit/d7e714ebab500156d6e5a3b5887173f9ea1e6402))

## [0.4.1](https://github.com/jolars/panache/compare/panache-formatter-v0.4.0...panache-formatter-v0.4.1) (2026-05-01)

### Bug Fixes
- **formatter:** extend block-token list ([`d087729`](https://github.com/jolars/panache/commit/d08772922a3b983612fb29e3f0a1ed90510a66ff)), closes [#238](https://github.com/jolars/panache/issues/238)
- **parser:** handle Pandoc emphasis on the IR path ([`afa0ef5`](https://github.com/jolars/panache/commit/afa0ef5e3a202dae86ff1b4a282618b35a34f413))
- **parser:** implement IR algorithm ([`bb91c85`](https://github.com/jolars/panache/commit/bb91c850dbf790895ab01e233aacde1debd544a5))
- **formatter,parser:** handle setext in list ([`86494b5`](https://github.com/jolars/panache/commit/86494b57765e2c2a8eae7b1183018774bd99fecc))
- maintain list markers for commonmark ([`084fc87`](https://github.com/jolars/panache/commit/084fc870805fa1fe8b4b36fcfe0c4b06f2a23a43))
- **parser:** support multiline setext headings ([`4b4e1a3`](https://github.com/jolars/panache/commit/4b4e1a3b90e78c8ca0b981051d68dbf33805faad))

## [0.4.0](https://github.com/jolars/panache/compare/panache-formatter-v0.3.1...panache-formatter-v0.4.0) (2026-04-29)

### Features
- add `Dialect` to untangle CommonMark from Pandoc ([`a1cb7df`](https://github.com/jolars/panache/commit/a1cb7df9ca8461f45db2b7f4efb50e57e8febce3))

### Bug Fixes
- **parser:** handle ruler as only list item ([`a1004e6`](https://github.com/jolars/panache/commit/a1004e66c6a4e6404ded859a997405e24d85eb3e))
- **parser:** handle autolinks and blockquotes for cmark ([`b1cedd4`](https://github.com/jolars/panache/commit/b1cedd4f586ea53b7174a039d37f2160c1dcdfab))
- **formatter:** ensure blankline before header in commonmark ([`fd96f2a`](https://github.com/jolars/panache/commit/fd96f2a016d8b3177122d8734bdb96b3db9188dd))
- handle thematic breaks in commonmark correctly ([`f98fca0`](https://github.com/jolars/panache/commit/f98fca002c517d06a67c443d4c1e841ebe087842))

## [0.3.1](https://github.com/jolars/panache/compare/panache-formatter-v0.3.0...panache-formatter-v0.3.1) (2026-04-27)

## [0.3.0](https://github.com/jolars/panache/compare/panache-formatter-v0.2.1...panache-formatter-v0.3.0) (2026-04-27)

### Features
- **cli:** make `--debug` actually useful in release builds ([`92a54ec`](https://github.com/jolars/panache/commit/92a54ecc087a10347a94fccfb7210dfdc345220f))

### Bug Fixes
- **formatter:** avoid quote character collisions ([`3c04c34`](https://github.com/jolars/panache/commit/3c04c3406eb4c84d1e1ef9a4dfe4051b33a6d111)), closes [#225](https://github.com/jolars/panache/issues/225)

## [0.2.1](https://github.com/jolars/panache/compare/panache-formatter-v0.2.0...panache-formatter-v0.2.1) (2026-04-24)

### Bug Fixes
- **formatter:** don't break display math inside emphasis ([`d2eee34`](https://github.com/jolars/panache/commit/d2eee343d1e5099ca28a7a7dec50fb4aa9ca5f0b)), closes [#214](https://github.com/jolars/panache/issues/214)
- **formatter:** handle nested lists with continuation ([`185fa02`](https://github.com/jolars/panache/commit/185fa022db7e4c231bfddbe6efd01062033e948a)), closes [#212](https://github.com/jolars/panache/issues/212)
- properly parse and format blockquote markers in deflist ([`b27eeb7`](https://github.com/jolars/panache/commit/b27eeb77aaf833aba1ab1370504b90b8a6e2d252)), closes [#209](https://github.com/jolars/panache/issues/209)
- **formatter:** strip whitespace from code in list ([`b1b60c0`](https://github.com/jolars/panache/commit/b1b60c0e6e39b12d3143fee605a68b9057310f23))

## [0.2.0](https://github.com/jolars/panache/compare/panache-formatter-v0.1.0...panache-formatter-v0.2.0) (2026-04-22)

### Features
- **formatter:** place table captions after the table ([`7d38d60`](https://github.com/jolars/panache/commit/7d38d604b314d2fb5645aea77fc34b1c2d23bdc7))
- **formatter:** use hanging indent for table captions ([`1234626`](https://github.com/jolars/panache/commit/1234626bce03c7e725426934ef5c289867e53137))
- **formatter:** use `:` as table caption prefix ([`618326a`](https://github.com/jolars/panache/commit/618326a97a5f1c2c178a2e2f508516f15b3d58d0))
- **formatter:** force one blankline after hashpipe options ([`68bba1b`](https://github.com/jolars/panache/commit/68bba1bec56cb0473a1de4b86c0f26f698a5f3fb)), closes [#115](https://github.com/jolars/panache/issues/115)

### Bug Fixes
- greedily consume table captions ([`58afc1c`](https://github.com/jolars/panache/commit/58afc1c2c27182a7e9768a1ff3f3b2b6e82531d5))
- **formatter:** correctly handle blanklines in blockquote ([`834757c`](https://github.com/jolars/panache/commit/834757c21a2844c27b46312a5a0ee0a7a003cc0d)), fixes [#199](https://github.com/jolars/panache/issues/199)
- **formatter:** handle blank line before fenced code ([`e7337fd`](https://github.com/jolars/panache/commit/e7337fdb4cece3a1cab45047b910cb43ac51efbc)), closes [#198](https://github.com/jolars/panache/issues/198)
- **formatter:** strip trailing whitespace in hashpipe flow ([`9757c2f`](https://github.com/jolars/panache/commit/9757c2fd16542f777e28c1cce3ce2b07e4f98d4d)), fixes [#194](https://github.com/jolars/panache/issues/194)
- **formatter:** quote ambiguous labels in hashpipe conversion ([`e473944`](https://github.com/jolars/panache/commit/e4739441e3443dc8f6f50174bea14897a6b16f9a)), closes [#192](https://github.com/jolars/panache/issues/192)
- avoid wrapping on fancy markers in unsafe contexts ([`4de13dd`](https://github.com/jolars/panache/commit/4de13dd0fe44b9bb728d7aa22b772a2267cf060b)), closes [#193](https://github.com/jolars/panache/issues/193)
- **formatter:** handle citation spacing correctly ([`543aa46`](https://github.com/jolars/panache/commit/543aa46cc0ebbe3073e1eeda01b04bb058cd9d66)), ref [#193](https://github.com/jolars/panache/issues/193)
- **formatter:** don't collapse whitespace in hashpipe yaml ([`5d4b5d2`](https://github.com/jolars/panache/commit/5d4b5d2f60ef85a0ba557c62804795bd22f6f378)), closes [#185](https://github.com/jolars/panache/issues/185)
- **formatter:** add list markers to unsafe wrappers ([`a7f1ed5`](https://github.com/jolars/panache/commit/a7f1ed514e33d956ca6892f9e6bf005f7c08ce6a)), closes [#187](https://github.com/jolars/panache/issues/187)
- **formatter:** normalize scalars to avoid idempotency issue ([`da9e3a0`](https://github.com/jolars/panache/commit/da9e3a0117bd152a1bb5407212168f0ed0640b17)), closes [#189](https://github.com/jolars/panache/issues/189)
