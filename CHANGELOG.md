# Changelog

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
