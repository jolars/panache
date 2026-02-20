# Changelog

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
