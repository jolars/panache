# Changelog

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
