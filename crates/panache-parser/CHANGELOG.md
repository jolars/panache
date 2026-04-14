# Changelog

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
