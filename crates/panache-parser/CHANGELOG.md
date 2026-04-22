# Changelog

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
