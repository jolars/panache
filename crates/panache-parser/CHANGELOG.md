# Changelog

## [0.27.1](https://github.com/jolars/panache/compare/panache-parser-v0.27.0...panache-parser-v0.27.1) (2026-08-20)

### Bug Fixes
- **formatter:** preserve R T and F aliases ([`153ea95`](https://github.com/jolars/panache/commit/153ea957138f7be00f3634ec13864da4217fb858))
- **parser:** trim bare URI punctuation ([`1c468d6`](https://github.com/jolars/panache/commit/1c468d6a3c46a71807726fe7404668f30807cd27))

### Performance Improvements
- **parser:** share block parser registry ([`7681de4`](https://github.com/jolars/panache/commit/7681de440923900c9ad7805bc63694f7f00c0181))

## [0.27.0](https://github.com/jolars/panache/compare/panache-parser-v0.26.0...panache-parser-v0.27.0) (2026-08-17)

### Features
- **parser:** add a bounded region tier, tried last ([`be854b2`](https://github.com/jolars/panache/commit/be854b2bcc4d6dbbe2c739b65f69a88d111ea898))
- **parser:** add a fragment parse entry point ([`c0c1df4`](https://github.com/jolars/panache/commit/c0c1df4b6d051877ccacbfcf3351853b36aa0c0a))
- **parser:** splice a single prose `TEXT` token in place ([`4f020ff`](https://github.com/jolars/panache/commit/4f020ff2abd01991357d58c82f5deb3eb1d4e052))

### Bug Fixes
- **parser:** decline splices below a retained definition list ([`c60535c`](https://github.com/jolars/panache/commit/c60535c2617cea92b4faddda7d5be32c4131290e))
- **pandoc-ast:** gate smart typography on the flavor ([`23bb54a`](https://github.com/jolars/panache/commit/23bb54a47ca784c5af28389dc58b021dd50cb1ee))
- **parser:** anchor yaml metadata delimiters at column 0 ([`55e3999`](https://github.com/jolars/panache/commit/55e3999730d4987bc37f369733ed0f420d9e6c40))
- **config:** disable `auto-identifiers` for `mdsvex` ([`21818ec`](https://github.com/jolars/panache/commit/21818ec918c55d8b38005e2cd279e844dd25891d))
- gate heading auto-ids on `auto_identifiers` ([`efd4ae0`](https://github.com/jolars/panache/commit/efd4ae053acdad06f0f559ffdff803fd77348bf8))
- **parser:** gate heading attrs on `header_attributes` ([`1291394`](https://github.com/jolars/panache/commit/1291394ee4edce57b65766119eacd4afd9e509c2))
- **parser:** close headings on a spaceless `#` run ([`16ce2a0`](https://github.com/jolars/panache/commit/16ce2a0fa9cddd7b3ef39e8041929078dabfe5d8))
- **parser:** read heading attrs after `#` run ([`52147de`](https://github.com/jolars/panache/commit/52147de9874eea75020ebcfd8cbbf5c301745731))
- **parser:** enable `escaped_line_breaks` for `gfm` ([`6a9c0f6`](https://github.com/jolars/panache/commit/6a9c0f6cd29579158b5c4e429167f393f183df09))
- **parser:** keep an escaped space out of the attribute gap ([`241310e`](https://github.com/jolars/panache/commit/241310ec9355c4f5ea058f66441015881730e6e7))
- **parser:** fold whitespace into a backslash line break ([`eaf42a0`](https://github.com/jolars/panache/commit/eaf42a00e77fe3bfda14a01e1568605f5b9c2785))
- **parser:** read a heading's trailing `\` as a line break ([`3ba1faf`](https://github.com/jolars/panache/commit/3ba1faf1624453560778175341ce6ae9e44f5c27))
- **parser:** keep trailing `\` literal in CommonMark ([`cab28d3`](https://github.com/jolars/panache/commit/cab28d3a857eb7a85640dc264a95c2c5ab8f9be9))
- **parser:** tighten whitespace hard-break rule ([`338743c`](https://github.com/jolars/panache/commit/338743c4c72e364e17802557eccda7e13167955b))

### Performance Improvements
- **parser:** promote the region tier ahead of the windows ([`1868fb0`](https://github.com/jolars/panache/commit/1868fb0509ffdeecc713349cabff0bb297e691a9))
- **parser:** stop reading the old text through the whole tree ([`cd1a769`](https://github.com/jolars/panache/commit/cd1a769dcba94dbb8faa96307e479aba32aed363))
- **bench:** measure the token tier, and keep the cutoff pair honest ([`6bedeb3`](https://github.com/jolars/panache/commit/6bedeb3e61abf377a02caac725472e0e0c6964bd))
- **parser:** serve backtick and space runs from a static pool ([`050f809`](https://github.com/jolars/panache/commit/050f809fe4ffd6d2e1e53eaa983a097331ab1368))
- **parser:** reuse the validator's YAML tree in `prepare_yaml_content` ([`7a1533b`](https://github.com/jolars/panache/commit/7a1533bb9293a272bb4f6b3e9626d89f35c0d406))
- **parser:** byte-gate `try_parse_horizontal_rule` before trim ([`1b7b417`](https://github.com/jolars/panache/commit/1b7b4178bd822c0b680da9b7da2d8723ee3db775))

## [0.26.0](https://github.com/jolars/panache/compare/panache-parser-v0.25.0...panache-parser-v0.26.0) (2026-08-14)

### Features
- **parser:** add pandoc 3.10 compat target ([`81a955b`](https://github.com/jolars/panache/commit/81a955b10a7c3e96dca165dec8731ad371bf3a7a))
- **parser:** tag container prefix as `LINE_PREFIX` ([`1d4ac7c`](https://github.com/jolars/panache/commit/1d4ac7c1720dd51328917b6747f84628d8637bcd))
- **parser:** introduce `FrameVerdict` frame resolution ([`a2d7446`](https://github.com/jolars/panache/commit/a2d7446381bfa010e108c80c2ab04e7afd5fe9b9))

### Bug Fixes
- **parser:** keep indent on a nested term line ([`b928aff`](https://github.com/jolars/panache/commit/b928affb049d16063bbe0c33b9270d1fd43340b5)), closes [#499](https://github.com/jolars/panache/issues/499)
- **parser:** keep a marker-only quote's newline ([`3d62176`](https://github.com/jolars/panache/commit/3d62176ab95dd943bdee5d3e63f69fc10a5f5a63))
- **parser:** open every quote marker on a list marker line ([`db197f3`](https://github.com/jolars/panache/commit/db197f3e357055c01682bbd931a3a25eec888795))
- **parser:** lift marker-line table in quoted item ([`ab1d0b2`](https://github.com/jolars/panache/commit/ab1d0b29b1b876dab52997e88a5b38128283f6a3))
- **parser:** keep a blank line's newline out of the prefix ([`58f3e20`](https://github.com/jolars/panache/commit/58f3e20f6aff489628fb9946c1d90fc8ffa8eb2c))
- **parser:** emit sibling item for drifted markers ([`74abcd5`](https://github.com/jolars/panache/commit/74abcd5773089159d4530fbca037d8dc19720aa3))
- **parser:** split row bands at hybrid sep lines ([`712fee3`](https://github.com/jolars/panache/commit/712fee3fd3fc2fca3c3323fe66d10dd4e9ab1297))
- **parser:** align nested definition body frames ([`bb1ead7`](https://github.com/jolars/panache/commit/bb1ead7cc661d75b5774a50f0b82fba8d3d0af32))
- **parser:** reject blank line under multiline opener ([`c8a8688`](https://github.com/jolars/panache/commit/c8a8688ca9d2189525e86123a0a8c8ba72a56d5f))
- **parser:** open multiline table on spaced border ([`f895545`](https://github.com/jolars/panache/commit/f895545f84f02656836e6f610e1273b4cccbc1d7))
- **parser:** nest ordered marker at content column ([`0f50724`](https://github.com/jolars/panache/commit/0f507241a08fbb117d87e2014d4978a6c1cdcadc))
- **parser:** emit quoted item's outer `>` in place ([`cd3d2b2`](https://github.com/jolars/panache/commit/cd3d2b2ce85377aae1eabe94e9f17ea0422e8d70))
- **parser:** keep lazy deeper `>` in quoted item text ([`6a6f242`](https://github.com/jolars/panache/commit/6a6f242a7d615fa786c84e8ada7e15b341903b62))
- **parser:** keep quoted band-marker ladders open ([`afa9021`](https://github.com/jolars/panache/commit/afa902132fd5125b91386375074144d69d1e1c48))
- **parser:** widen list-start fence to nested bands ([`f26180d`](https://github.com/jolars/panache/commit/f26180d691cd7b2bb0a8f9a0ddd7feabaa764ff8))
- **parser:** apply table footer rule at run ends ([`89bc3cd`](https://github.com/jolars/panache/commit/89bc3cdb7403fea2fcd53ae06a7abd8dfc1c0e9e))
- **parser:** open blocks on non-bare note marker lines ([`75a5637`](https://github.com/jolars/panache/commit/75a5637125f5d7ec7f3de76d5685cb13fd0dc640))
- **parser:** never read a thematic break as a list marker ([`de9e117`](https://github.com/jolars/panache/commit/de9e117e563284e6ffa57295fcd75ee551a3ad23))
- **parser:** stop duplicating footnote indent in quoted tables ([`59b0fe9`](https://github.com/jolars/panache/commit/59b0fe97ae0521ab7e9d04049138b9ecde7482f9))
- **parser:** keep rowspan grids whole in containers ([`2e5ac3b`](https://github.com/jolars/panache/commit/2e5ac3b1f5aec13e97b7a7d26358ee0f54a57124))
- **parser:** lift ATX and HTML in quoted items ([`72101b8`](https://github.com/jolars/panache/commit/72101b858ca9dc1289b439f2fe6379e91ce97d79))
- **parser:** skip line prefixes in grid table projection ([`928393b`](https://github.com/jolars/panache/commit/928393b73400ef9158d1b35759d1b3a29bd7855f))
- **parser:** fold setext underline in quoted list item ([`89f7923`](https://github.com/jolars/panache/commit/89f79231aa38be542db83402b289ab694cc59c3d))
- **parser:** lift a quoted item's marker-line table ([`f4cf348`](https://github.com/jolars/panache/commit/f4cf3481b0ae13c8a0a69fff6cfb03cd57fa56b0))
- **parser:** take pipe table columns from the delimiter row ([`d194da2`](https://github.com/jolars/panache/commit/d194da21162ba97d4a98fec598e69a6d4f097849))
- **parser:** let a marker-shaped delimiter row finish its table ([`6f4787e`](https://github.com/jolars/panache/commit/6f4787e92ed8c6dcaebd7c7a7725c382bb61da8d))
- **parser:** bound pipe table rows at `nonindentSpaces` ([`a7c3e9b`](https://github.com/jolars/panache/commit/a7c3e9b3e28c33e6110111e83591acfbe5b5d8dc))
- **parser:** open a container body with a pipe table ([`0e11028`](https://github.com/jolars/panache/commit/0e11028bde7eb5ce82ca0c0fcfa3f32a56e079d7))
- **parser:** match pandoc's `alignType` in the projector ([`1c81dfd`](https://github.com/jolars/panache/commit/1c81dfd2daaa36f936663beaed12d9a6e5385872))
- **parser:** let a quoted div hold a nested blockquote ([`cbc3a9c`](https://github.com/jolars/panache/commit/cbc3a9c8d1e5ac5bb8a56103191d2cd3f5e10236))
- **parser:** stop `ListAdvance` eating content bytes ([`036d786`](https://github.com/jolars/panache/commit/036d786396544bc480e93342b2249c29c620bad2))
- **parser:** keep non-interrupting HTML in definition PLAIN ([`c1beb1b`](https://github.com/jolars/panache/commit/c1beb1b8749dcdfd0509892a1b57d78833e5fcae))
- **parser:** break lazy quote text at html blocks ([`97c47df`](https://github.com/jolars/panache/commit/97c47df34d367cd8558deac172a0ec69686cde79))
- **parser:** fix line-0 strip in html tag scan ([`e40664f`](https://github.com/jolars/panache/commit/e40664fbc7dedfebd3fd23243939023e9bca5372))
- **parser:** align the backward caption scan with the seam ([`2842258`](https://github.com/jolars/panache/commit/284225890f8dbe497e6a9dd9347da68820adddc4))
- **parser:** accept table closers abutting a run terminator ([`690831e`](https://github.com/jolars/panache/commit/690831eddc1aef0aad955b0d4ef2a6fe7e71e284))
- **parser:** bound forward caption scans on the seam ([`09ce313`](https://github.com/jolars/panache/commit/09ce31341acb90364a86db4cd8cc3a148c79cc9b))
- **parser:** bound the multiline-table scan on the seam ([`9d706f5`](https://github.com/jolars/panache/commit/9d706f555fc16ed2c572af93d40f3cf86087ca69))
- **parser:** fence table scans at innermost div closers ([`7a93e49`](https://github.com/jolars/panache/commit/7a93e490feb6a268381df8bb92134bf9a7aed7f2))
- **parser:** bound pipe-table rows at container ends ([`d476f37`](https://github.com/jolars/panache/commit/d476f377721cd2539644e13f847faadeeaa3466f))
- **parser:** end container line runs at html closers ([`515dc91`](https://github.com/jolars/panache/commit/515dc91a673628fc70a34122cac57892dc76610e))
- **parser:** end container line runs at new list starts ([`8e892b3`](https://github.com/jolars/panache/commit/8e892b3ba9511f95bc4c58b5f532e433c6478d9b))
- **parser:** end container line runs at note markers ([`a698fab`](https://github.com/jolars/panache/commit/a698fabbbfb48f1e7f94fd269fb73d4ee03fcb5e))
- **parser:** end table scans at div closers ([`f376148`](https://github.com/jolars/panache/commit/f3761480206901f8e2adfe7925a740c73d579668))
- **parser:** caption tables in footnote bodies ([`6244912`](https://github.com/jolars/panache/commit/6244912535b4c7721f004adc684a469c1298daab))
- **parser:** detect self-indented simple tables in list items ([`9987777`](https://github.com/jolars/panache/commit/99877772d65bc007deda29a269cb81b774c3fcbb))
- **parser:** lossless tables in definition bodies ([`6effc2a`](https://github.com/jolars/panache/commit/6effc2a01ec78a2ea9c9ab1938d35c8f1c9fe6e0))
- **parser:** read definition markers behind a straddling tab ([`2035a69`](https://github.com/jolars/panache/commit/2035a6973da452c6f9148cc0af0e45b2a20f60e5))
- **parser:** bound caption probe to its container ([`b31f2d6`](https://github.com/jolars/panache/commit/b31f2d6e3d264e1c9a66d84b7da78c7a237fdfc0))
- **parser:** detach a term across two blank lines ([`730b490`](https://github.com/jolars/panache/commit/730b490711a50d5d61932d33ce22ce4992163eac))
- **parser:** promote a term across a blank line ([`cf392b2`](https://github.com/jolars/panache/commit/cf392b2c5f0ee6dd5b7b6d13d3484a21af4592dd))
- **parser:** promote a one-line body to a term ([`952007e`](https://github.com/jolars/panache/commit/952007ecfff515f1ee111d68dc22900ca3c15f8e))
- **parser:** end a definition body block at a `:` marker ([`928af1a`](https://github.com/jolars/panache/commit/928af1a0c1f774995812c15069bcd09a166d0bc1))
- **parser:** end a list item block at a `:` marker ([`d2ed198`](https://github.com/jolars/panache/commit/d2ed1984203d5b8be1ba3fa033ef0b2607a3e4fb))
- **math:** keep `:=` glued as one relation ([`a65bb40`](https://github.com/jolars/panache/commit/a65bb4035a29a31d648f6d041763c60c53a5da47)), closes [#487](https://github.com/jolars/panache/issues/487)
- **parser:** open indented code below a closing fence ([`f7f5e34`](https://github.com/jolars/panache/commit/f7f5e34d07fbeec9d2d254672cce380ae4ced0c7)), closes [#471](https://github.com/jolars/panache/issues/471)
- **parser:** keep a fence's closer scan inside its own container ([`ed96588`](https://github.com/jolars/panache/commit/ed9658813dddbc0d8b8683e02ae2b497912097c1))
- **parser:** support `-` as `.unnumbered` shorthand ([`f8f6e67`](https://github.com/jolars/panache/commit/f8f6e67515c4b10a3a4de16c8a8d54d7b8325a47)), closes [#467](https://github.com/jolars/panache/issues/467)
- **parser:** keep the item separator out of a nested definition list ([`709c74f`](https://github.com/jolars/panache/commit/709c74fc58261aac1a8c542be4eedc520a94447c))
- **parser:** nest a definition list inside its list item ([`c143a96`](https://github.com/jolars/panache/commit/c143a96c9d2fd20292a6c663deca9c061e6aad14))
- **parser:** read definition markers at the item content column ([`d154e1f`](https://github.com/jolars/panache/commit/d154e1f1fc2f7cee5e75a1417bd0db0e3ad0e85f))
- **parser:** require a definition term to be a one-line block ([`9ff8596`](https://github.com/jolars/panache/commit/9ff85960fbdd8919e5c8f60acb4b76272ec88c91))
- **parser:** gate definition term lookahead on real indent ([`c5a4dc2`](https://github.com/jolars/panache/commit/c5a4dc25c58e916908f01dbb69ad3ecdd95d25cf))
- **parser:** let a closed bare fence interrupt a para ([`6504857`](https://github.com/jolars/panache/commit/6504857ca528e3a3afdcaf8cec9539c1886bca97))
- **parser:** end fence closer scan at list item end ([`edf05b4`](https://github.com/jolars/panache/commit/edf05b4d1f34b05a41f4bdd2509a5cff40ecb4a0))
- **formatter:** expand code-span tabs from source column ([`ff7dafa`](https://github.com/jolars/panache/commit/ff7dafa924e80cc04e8b2c271151c0bbb7173e7d))
- **parser:** gobble list indent per container level ([`94a7eaf`](https://github.com/jolars/panache/commit/94a7eaf8567af8c73ff9a7ee4d2eb654f2503f0e))
- **parser:** fold lazy `>` into list-item text ([`0041501`](https://github.com/jolars/panache/commit/00415013a1d1dcb6e4e49a25ce2ad3580af10e30))
- **parser:** implement pandoc's `compactify` for lists ([`f015f78`](https://github.com/jolars/panache/commit/f015f782b1aee9d1a2ffb0554e40cb81fdaf5769))
- **parser:** require content after a task checkbox ([`4153014`](https://github.com/jolars/panache/commit/4153014477850f8810a52cd57c99d6dde4f2ac9a))
- **parser:** open a footnote def at a list item's column ([`1d0270d`](https://github.com/jolars/panache/commit/1d0270da87d9fcfd685da65ce361fa07861f5920))

### Performance Improvements
- improve and harden incremental parser (#486) ([`70e5750`](https://github.com/jolars/panache/commit/70e5750dbab1034a08e3b08bc6114c1f1797f7ae))

## [0.25.0](https://github.com/jolars/panache/compare/panache-parser-v0.24.0...panache-parser-v0.25.0) (2026-08-07)

### Features
- **parser:** decode entities in HTML attributes ([`88103d6`](https://github.com/jolars/panache/commit/88103d64871ad4767916546c529f5f38c8f72eda))

### Bug Fixes
- **parser:** gobble a footnote body's indent ([`6ec04f5`](https://github.com/jolars/panache/commit/6ec04f57851382e2c0c356a6dbac3c3598e8529d))
- **parser:** gobble a definition body's indent ([`f7cdf8a`](https://github.com/jolars/panache/commit/f7cdf8a21c45328c7801c2a247c9623760b47133))
- **parser:** expand a code span's tabs by source column ([`33a5e4a`](https://github.com/jolars/panache/commit/33a5e4a8d02d722e150c055f2454c81382a3fcd0))
- **parser:** gobble a list item's continuation indent ([`42429b0`](https://github.com/jolars/panache/commit/42429b0bb70df7b721b52496fd8bde50790f881b))
- **linter:** make `missing-bibliography-key` more precise ([`362d183`](https://github.com/jolars/panache/commit/362d18317e6a10129b35df7733595519106a0401))
- **parser:** keep an over-indented fence lazy ([`00f5f44`](https://github.com/jolars/panache/commit/00f5f441fb7a104e77b64602863e652c8c2e2e73))
- strip a fenced block's own indent from its payload ([`290e3ab`](https://github.com/jolars/panache/commit/290e3abb1f04dbad29451c3abe70aa6a6f8738e0))
- **parser:** open a lazy fence in a quoted list item ([`ee3167c`](https://github.com/jolars/panache/commit/ee3167ceb37acc3631acc9d723ef45f3482ed740))
- **parser:** strip container prefix from code payloads ([`e31f0b9`](https://github.com/jolars/panache/commit/e31f0b9489619d24783ddb4642d7c6f79bafe084))
- **parser:** loosen a list on a para-breaking block ([`fc14e4f`](https://github.com/jolars/panache/commit/fc14e4f826da2d5d22a582f7f66c484e7042eeaf))
- **parser:** gobble every lazy line in a blockquote ([`354eafe`](https://github.com/jolars/panache/commit/354eafe33086cc0a101e89b085b5760cb999767c))
- **parser:** open a line block on a list-marker line ([`5f55d89`](https://github.com/jolars/panache/commit/5f55d89ab91d29ebfa41a9facf6fb469ed05e1ed))
- **parser:** fold an indented line-block marker ([`e564273`](https://github.com/jolars/panache/commit/e56427397b26ff6d67def2fca15d7b6629b278c6))
- **parser:** gate the setext-after-setext escape ([`729098f`](https://github.com/jolars/panache/commit/729098f4c0a2055560e4a0df39ac2a4f6dbcd098))
- **parser:** fold a lazy line into its blockquote ([`03d1339`](https://github.com/jolars/panache/commit/03d13398d0ead663bad983b7e02ef6e8e6d38575))
- **parser:** fold a lazy line into its line block ([`7d732b9`](https://github.com/jolars/panache/commit/7d732b91ab4cecb48d6868edb73d78c0b7b6bf5c))
- **parser:** keep a quoted definition list intact ([`b361fe0`](https://github.com/jolars/panache/commit/b361fe04a4377ea0c1f5861e687d735e8b3aa82e))
- **parser:** accept a pipe table with no body rows ([`070f20a`](https://github.com/jolars/panache/commit/070f20a5981bcea6689ac7d02a3f37533d70f717))
- **parser:** cap blockquote depth by registry rank ([`fcc0f9f`](https://github.com/jolars/panache/commit/fcc0f9f4ca87d8b277654eb0f3c30f2109aee9e4))
- **parser:** apply the setext same-container rule to pandoc ([`489510e`](https://github.com/jolars/panache/commit/489510e4059922aa234e04df20a3beee1af5ad30))
- **parser:** count underline markers on the raw line ([`7f9e9c3`](https://github.com/jolars/panache/commit/7f9e9c3a91b25af5b3e04d342aeb1e0e4bfafca1))
- **parser:** let the registry's verdict outrank `>` count ([`5d97a32`](https://github.com/jolars/panache/commit/5d97a32b2fc4f1d2e172fded2e0ee1dfa3d9a8a0))
- **parser:** keep div opener whitespace before label ([`864cc90`](https://github.com/jolars/panache/commit/864cc903e6625b48e93f5e8e693596b3540dfa3f))
- **parser:** stop dropping surplus display-math dollars ([`33b6404`](https://github.com/jolars/panache/commit/33b64047b5e39f53f46602942234c61e36d95ec3))
- **parser:** keep `:::` openers from interrupting list items ([`37d95e1`](https://github.com/jolars/panache/commit/37d95e1106bcde0f6e20588f167bec6268f7128f))
- **parser:** match line-block peek to emitted prefix ([`f7d50ee`](https://github.com/jolars/panache/commit/f7d50eeb7b2889072e2b0fa2a5e793457353c768))
- **parser:** emit blockquote setext from stripped lines ([`985aba2`](https://github.com/jolars/panache/commit/985aba26f812dbb2a75a4d1129a1a5ca13b2d79b))
- **parser:** let setext claim a quoted line at top level ([`c60043c`](https://github.com/jolars/panache/commit/c60043cb290958c9d7c7a2bd0dd6a2826f4cb024))
- **parser:** keep refdefs from interrupting list items ([`6f36287`](https://github.com/jolars/panache/commit/6f3628743614dfdbbb078e2161284c21c18ca469))

## [0.24.0](https://github.com/jolars/panache/compare/panache-parser-v0.23.0...panache-parser-v0.24.0) (2026-08-05)

### Features
- **linter:** add `unsupported-metadata-key` rule ([`8056ffe`](https://github.com/jolars/panache/commit/8056ffe28cc165df52a9f29d62e75586f8a5f9c0))

### Bug Fixes
- **parser:** stop reading `^[note][`/`(` as a footnote ([`e69e8c7`](https://github.com/jolars/panache/commit/e69e8c7172f7cf5b4996a80ea75d39477ba4c4b2)), refs [#455](https://github.com/jolars/panache/issues/455)
- **parser:** reword the empty YAML key diagnostic ([`73cc983`](https://github.com/jolars/panache/commit/73cc983be4d105b4e270e8d8e8db2bb595049bb8))
- **parser:** promote figures in list items and definitions ([`05a6358`](https://github.com/jolars/panache/commit/05a63584682ae6dee6319610b589ffb2e5780b33))
- **parser:** promote figures only at paragraph close ([`70d8cbe`](https://github.com/jolars/panache/commit/70d8cbe7ded58ff66f99f8b96955aad25e3edf0b))

## [0.23.0](https://github.com/jolars/panache/compare/panache-parser-v0.22.2...panache-parser-v0.23.0) (2026-08-03)

### Features
- **parser:** recognize bare `:` definition marker ([`cfa9eec`](https://github.com/jolars/panache/commit/cfa9eec19c010a810e906424040736067d036192))
- **linter:** add `unspaced-citation` rule ([`b36baed`](https://github.com/jolars/panache/commit/b36baedc4902c55f7bbfce151a3e0dcfde422c0e)), closes [#448](https://github.com/jolars/panache/issues/448)
- **parser:** reject undeclared YAML alias ([`4dbdfe6`](https://github.com/jolars/panache/commit/4dbdfe6e6aa2f3f298221c2f4e3f19772608cf33))
- **parser:** lift unclosed `<div>` as list-item body ([`4801192`](https://github.com/jolars/panache/commit/48011921ad9ec24041a9f647f7ecfaa086e4b8d4))
- **parser:** lift unclosed `<div>` on later content-body line ([`151515d`](https://github.com/jolars/panache/commit/151515da464940007eb32a66055b3340ad9319a4))

### Bug Fixes
- **parser:** require a term before a definition marker ([`cc8a4fc`](https://github.com/jolars/panache/commit/cc8a4fc4b71eab6cd11a5760d66102ef6a6579cf))
- **parser:** keep def-list para open across `>` line ([`cbb55c4`](https://github.com/jolars/panache/commit/cbb55c41021e289438e475b47751740c79d05a6c))
- **parser:** suppress bare `@key` after emphasis closer ([`d6a3f87`](https://github.com/jolars/panache/commit/d6a3f8702e926769452bdd340616b3ed7f78bd15))
- **parser:** don't cite bare `@key` after a word char ([`291efec`](https://github.com/jolars/panache/commit/291efec5d4d3a6f571e7e69f36181dd9ba3319aa))
- **parser:** lift bq-nested def later-line HTML block ([`d4549fd`](https://github.com/jolars/panache/commit/d4549fd3ae66f52723df30e3a5772b97fd2f6cbb))
- **parser:** don't retag `HTML_BLOCK_DIV` in bq content body ([`34d9a4a`](https://github.com/jolars/panache/commit/34d9a4aa75ccfdcc2a31328c6e28f69b7fe2a344))
- **parser:** don't close top-level div on indented fence ([`3a603e1`](https://github.com/jolars/panache/commit/3a603e1870c48d7305cf279faaaa49e7edc27bb3))

## [0.22.2](https://github.com/jolars/panache/compare/panache-parser-v0.22.1...panache-parser-v0.22.2) (2026-07-28)

### Bug Fixes
- stabilize divs and code blocks in list items ([`3c47658`](https://github.com/jolars/panache/commit/3c476588af1cbdcf1e9f002a5dc904373fbc800e)), closes [#439](https://github.com/jolars/panache/issues/439)
- **parser:** inline-parse single-column multiline table cells ([`f69d2bb`](https://github.com/jolars/panache/commit/f69d2bbc7847a15f67c1b5322f9d529962fe0521)), closes [#438](https://github.com/jolars/panache/issues/438)

## [0.22.1](https://github.com/jolars/panache/compare/panache-parser-v0.22.0...panache-parser-v0.22.1) (2026-07-25)

### Bug Fixes
- **parser:** reject simple tables with only a closer ([`b926900`](https://github.com/jolars/panache/commit/b9269008989d63edc3b406d587252c710f43a5e8))
- **parser:** emit simple table closer as separator ([`0db2620`](https://github.com/jolars/panache/commit/0db2620eb1e34ac86c7e2c0eae070f4d9f39be30))
- **parser:** require closer for headerless simple tables ([`83dd8c5`](https://github.com/jolars/panache/commit/83dd8c526325cab3a33caba9da6a01b3a16ad330))
- **parser:** accept 2-dash multiline table borders ([`9be4e59`](https://github.com/jolars/panache/commit/9be4e597c4d45b4ccfc62e79e7df1852d8472d37))
- **parser:** detect headerless single-column multiline tables ([`a573220`](https://github.com/jolars/panache/commit/a57322065d8ff3b26bde55c990f728a56a42aab7))
- **parser:** track display math inside list items ([`6708303`](https://github.com/jolars/panache/commit/67083031fa8f89429510ae5164b22c9ffc769152))
- **parser:** track bracket display math across lines ([`cb6b70b`](https://github.com/jolars/panache/commit/cb6b70b55c25036415e29dfc4ab27c8e8c2c3f98)), closes [#437](https://github.com/jolars/panache/issues/437)
- **parser:** skip link destinations in citation scan ([`46d2201`](https://github.com/jolars/panache/commit/46d220132e53a52dd164b291f360f9e979a765c9))

## [0.22.0](https://github.com/jolars/panache/compare/panache-parser-v0.21.0...panache-parser-v0.22.0) (2026-07-20)

### Breaking changes
- remove long-deprecated config, CLI, and API surface ([`6af736d`](https://github.com/jolars/panache/commit/6af736d8ab38ecebfaa62d40fbfbe83a2a300adf))

### Features
- remove long-deprecated config, CLI, and API surface ([`6af736d`](https://github.com/jolars/panache/commit/6af736d8ab38ecebfaa62d40fbfbe83a2a300adf))
- **parser:** lift later-line HTML block in def/footnote body ([`21d448a`](https://github.com/jolars/panache/commit/21d448a3972261d6ec8472a0ccc74cd37f75688b))
- **parser:** fuse comment trailing softbreak in def body ([`1a2c8b9`](https://github.com/jolars/panache/commit/1a2c8b9728dae63989d7eddc3b348b85f8ae1f71))
- **parser:** lift HTML block on footnote marker line ([`cab5f61`](https://github.com/jolars/panache/commit/cab5f6125e51c22a07d7cfe3768885f3d4299b45))
- **parser:** dispatch HTML block on definition marker line ([`8eb6f6a`](https://github.com/jolars/panache/commit/8eb6f6a4657e807df5da154f169b50dbe9d4beb0))
- **parser:** fuse comment softbreak in blockquote ([`8a29570`](https://github.com/jolars/panache/commit/8a295706c62421157c59ea4282e49a2c15c4660b))
- **parser:** fuse comment softbreak in fenced div ([`baccde0`](https://github.com/jolars/panache/commit/baccde0247c1344c49a97c809c26f4a067a8728c))

### Bug Fixes
- **parser:** let blocks end reduced-marker lazy lines ([`edd18ee`](https://github.com/jolars/panache/commit/edd18eeeac33f2dcfd5d443192d387e6d3e27231)), closes [#429](https://github.com/jolars/panache/issues/429)
- **parser:** let ATX heading end a lazy blockquote line ([`71f46a4`](https://github.com/jolars/panache/commit/71f46a4bd8d971ec9d73d09d8bbcb220827d7779)), closes [#428](https://github.com/jolars/panache/issues/428)
- **parser:** keep setext from interrupting a paragraph ([`a1b9fa1`](https://github.com/jolars/panache/commit/a1b9fa1ff82c5f0d4f953413a08eaa3b12695e68))
- **parser:** preserve order for interrupting ATX headings ([`6620829`](https://github.com/jolars/panache/commit/662082943cd049aba7427d20975a383abe929839))
- **parser:** detect headerless single-column simple tables ([`0de4afc`](https://github.com/jolars/panache/commit/0de4afcbe0ecb23b1d5581078b2107dafa7a9a7b))
- **parser:** gate YAML metadata on top-level mapping ([`8f135bc`](https://github.com/jolars/panache/commit/8f135bc760e0961f89d8b6d7735e1f4526005ae0))
- **parser:** restrict mid-document YAML to pandoc dialect ([`53fabd9`](https://github.com/jolars/panache/commit/53fabd92e163272212fc9387bd82fc6e54b886aa))
- **parser:** detect rules and headings in nested list items ([`4de51b7`](https://github.com/jolars/panache/commit/4de51b78a4ae7235c6307383f870518e42862e0f))
- **parser:** lift multi-line `<div>` body on definition marker line ([`9a78ab8`](https://github.com/jolars/panache/commit/9a78ab893532143cbefd69c74042b01d01241eb6))

## [0.21.0](https://github.com/jolars/panache/compare/panache-parser-v0.20.0...panache-parser-v0.21.0) (2026-07-04)

### Features
- **parser:** fuse comment/PI trailing softbreak lines ([`3fa513c`](https://github.com/jolars/panache/commit/3fa513ce3dc2fd72abb1a2ca739d876393467505))
- **parser:** lift `<div>` inter-tag text to sibling blocks ([`5f04265`](https://github.com/jolars/panache/commit/5f04265dfb5a77e56fe9f448f159261297b9f80d))
- **parser:** lift fenced div swallowing html `</div>` ([`7b11daf`](https://github.com/jolars/panache/commit/7b11daf64a0fabb6fd4774006971aef387a9c259))
- **parser:** split same-line matched-pair inter-tag HTML ([`119014c`](https://github.com/jolars/panache/commit/119014c7f9f90a3d38d36c37562d37f4bfd1dd29))
- **parser:** classify void strict-block HTML tags ([`fcfea15`](https://github.com/jolars/panache/commit/fcfea15ce69d9a44fe2dcc0f18b29a86e2a7b3fc))
- **parser:** split blockquote standalone HTML tags ([`b29d46c`](https://github.com/jolars/panache/commit/b29d46c032c430bbc8e5ad561df209ffea00262c))
- **parser:** lift blockquote open-only HTML body ([`e3b93a5`](https://github.com/jolars/panache/commit/e3b93a50453174cc5a6ba855d673c680b4823f57))
- **parser:** flag unbalanced `\left`/`\right` in math ([`73750c9`](https://github.com/jolars/panache/commit/73750c9b854e8899fcd2b7180c21f9b2eb7af892))
- **parser:** lift open-only HTML block bodies ([`7a831ff`](https://github.com/jolars/panache/commit/7a831ff881c995cc9dfa57fb2aa6650699c431d8))

### Bug Fixes
- **parser:** map simple-table columns by display width ([`c2f1b14`](https://github.com/jolars/panache/commit/c2f1b141894cf66c323831e8100e4118529a8496)), fixes [#411](https://github.com/jolars/panache/issues/411)
- **linter:** match reference labels on raw source text ([`cb7ae6d`](https://github.com/jolars/panache/commit/cb7ae6d006d62d439f3df0e431186858d355c2f2))

## [0.20.0](https://github.com/jolars/panache/compare/panache-parser-v0.19.1...panache-parser-v0.20.0) (2026-07-01)

### Features
- **parser:** add MyST AST wrappers ([`e5d7ba8`](https://github.com/jolars/panache/commit/e5d7ba838d4afda1cc6ede8b20b5274dbb50f622))
- **parser:** treat standalone Svelte spans as opaque blocks ([`414d441`](https://github.com/jolars/panache/commit/414d441f8990f8874a604b63fd13b50fa5ce0564))
- **parser:** align myst defaults with myst-parser ([`4232185`](https://github.com/jolars/panache/commit/4232185235ffb74b898c0332e9e8400bd98f88a9))
- **parser:** parse myst verbatim-bodies ([`2d8a516`](https://github.com/jolars/panache/commit/2d8a51622766163b5963626ea4fe38d299179d47))
- **parser:** parse MyST directive option blocks ([`17990eb`](https://github.com/jolars/panache/commit/17990eb07e397762f28e2365c95c064dc590cba1))
- **parser:** add MyST flavor scaffolding ([`b4bdd84`](https://github.com/jolars/panache/commit/b4bdd84f56d8957583b1eb2ca4527d0d4952c1a5))
- **parser:** split standalone block-tag sequences in CST ([`9bf0005`](https://github.com/jolars/panache/commit/9bf00052b23f594281b9ef0f0025556ea1ae58b4))
- **parser:** gate YAML tab-indent diagnostics per consumer ([`dffefc8`](https://github.com/jolars/panache/commit/dffefc8d391888f08fbcbafb3200c0034e4bbe9d))
- add python-markdown admonitions and pymdownx details ([`b37a5cc`](https://github.com/jolars/panache/commit/b37a5cc2887029953eeb44b673a2fed39f3550be)), fixes [#396](https://github.com/jolars/panache/issues/396)
- add mdsvex flavor with opaque Svelte template spans ([`983632a`](https://github.com/jolars/panache/commit/983632ac09ff66c469f74d834452773f59f8a960))

### Bug Fixes
- **parser:** treat `{toctree}` body as verbatim ([`1ad3e87`](https://github.com/jolars/panache/commit/1ad3e878d63b967db18a083e8c622f00039a6132))
- **parser:** open brace-info code fences in CommonMark ([`179cb1c`](https://github.com/jolars/panache/commit/179cb1c6f4b876708d48cda827594b0282ff4b90))
- **parser:** span inline math across a newline (Pandoc only) ([`9b7905e`](https://github.com/jolars/panache/commit/9b7905eba103ff61d0ecbcf624e00e8032e036d2))
- **parser:** span inline math across a single newline ([`ace2ab4`](https://github.com/jolars/panache/commit/ace2ab429ddc017e235247a9bd077d6cdf8b199d))
- **parser:** parse headerless multiline tables with a dash-run closer ([`ab7e7d3`](https://github.com/jolars/panache/commit/ab7e7d315433e6e11d220c2735d2e7d4d884c10a)), fixes [#398](https://github.com/jolars/panache/issues/398)
- **parser:** require left word boundary for bare URIs ([`f7b6334`](https://github.com/jolars/panache/commit/f7b6334c45902592955e5a8bb24b9545f2ba7223))

## [0.19.1](https://github.com/jolars/panache/compare/panache-parser-v0.19.0...panache-parser-v0.19.1) (2026-06-24)

### Bug Fixes
- **parser:** emit bare URIs as lossless `AUTO_LINK` ([`52226d5`](https://github.com/jolars/panache/commit/52226d59067843251c71620dec29f25ffc9bcb07))
- **linter:** trim trailing newline from schema value spans ([`d1fedb9`](https://github.com/jolars/panache/commit/d1fedb9ef2dc3d88f99dc710422cc8f6076b9721))
- **parser:** stop truncating wide simple-table cells ([`f97694a`](https://github.com/jolars/panache/commit/f97694aeedeaf9913d31853c51025a27565ae68a))
- **parser:** consume top border of single-row multiline tables ([`0872624`](https://github.com/jolars/panache/commit/0872624d745fa56e11aa493ba41dc452b72818da))

## [0.19.0](https://github.com/jolars/panache/compare/panache-parser-v0.18.0...panache-parser-v0.19.0) (2026-06-23)

### Features
- add `crossref-prefixes` for extension crossrefs ([`0b190cc`](https://github.com/jolars/panache/commit/0b190cc1ad00e1ca146c758226a325b0c7a16017))

### Performance Improvements
- **parser:** de-duplicate definition marker parsing ([`91a1f10`](https://github.com/jolars/panache/commit/91a1f10bc53d92294082a2acfc3442487f49ad2d))

## [0.18.0](https://github.com/jolars/panache/compare/panache-parser-v0.17.2...panache-parser-v0.18.0) (2026-06-21)

### Features
- **parser:** retag comment/PI/verbatim as `HTML_BLOCK_RAW` ([`447d537`](https://github.com/jolars/panache/commit/447d537dcbd6bdff6f55d95cb04b17cd9fd17574))
- **parser:** tokenize table separator rows in the CST ([`3de91a6`](https://github.com/jolars/panache/commit/3de91a623762282b07ede9d249cf3872a5634a5f))

### Bug Fixes
- **parser:** stop caption theft across simple/multiline tables ([`89610d4`](https://github.com/jolars/panache/commit/89610d4c8230f4894bfe8552322403c54a7ed120))
- **parser:** skip code spans in citation detection ([`859a3db`](https://github.com/jolars/panache/commit/859a3dbbbb6a37b190c87187d85ab790e397f539))
- **parser:** detect setext heading level via underline node ([`ff43f66`](https://github.com/jolars/panache/commit/ff43f664bdb80af8b1452c286650609a250d865e)), closes [#377](https://github.com/jolars/panache/issues/377)

### Performance Improvements
- **parser:** replay table subtree instead of re-parsing ([`4a050dc`](https://github.com/jolars/panache/commit/4a050dccd45954b9b79ef0245dd82785988e976a))
- **parser:** use `memchr` for refdef newline scan ([`0e64ba7`](https://github.com/jolars/panache/commit/0e64ba700a3a2afbda06ae8755ab41e81cc0f171))

## [0.17.2](https://github.com/jolars/panache/compare/panache-parser-v0.17.1...panache-parser-v0.17.2) (2026-06-17)

### Bug Fixes
- **parser:** conform ordered marker on pipe-table line to pandoc ([`bf02d16`](https://github.com/jolars/panache/commit/bf02d1626c3f7f10de07e9ca735dfdb2de3d2759))

## [0.17.1](https://github.com/jolars/panache/compare/panache-parser-v0.17.0...panache-parser-v0.17.1) (2026-06-15)

### Bug Fixes
- **parser:** claim trailing caption for table-first list item ([`a09f066`](https://github.com/jolars/panache/commit/a09f066a9493f3f626b44691023c59c151caafb8))

## [0.17.0](https://github.com/jolars/panache/compare/panache-parser-v0.16.0...panache-parser-v0.17.0) (2026-06-13)

### Features
- **parser:** tokenize math delimiters and punctuation ([`7249710`](https://github.com/jolars/panache/commit/7249710c2c983f651358488b991c15d095b256ba))

### Bug Fixes
- **parser:** claim caption-led table as list item's first line ([`ac1f18b`](https://github.com/jolars/panache/commit/ac1f18b2b0a9e89d453f2a2f02fa91170c54fcb8))
- **parser:** keep table captions lossless inside containers ([`b04f65a`](https://github.com/jolars/panache/commit/b04f65aa33a365faf01bed3eff130e0f4760ba92))
- **parser:** treat deep list marker as lazy continuation ([`3fe17e0`](https://github.com/jolars/panache/commit/3fe17e09cb9515f7734f57b1013dc16181985816))

### Performance Improvements
- **parser:** make giant-blockquote parsing O(n) not O(n²) ([`4ec2cc0`](https://github.com/jolars/panache/commit/4ec2cc0011116fdba2bab588416cd636bac8445f))

## [0.16.0](https://github.com/jolars/panache/compare/panache-parser-v0.15.0...panache-parser-v0.16.0) (2026-06-10)

### Features
- **parser:** condition YAML validation on consumer profiles ([`f77f153`](https://github.com/jolars/panache/commit/f77f153a7ad8d3339e31395f2b9b76535c01dff5))

### Bug Fixes
- **parser:** report YAML required-simple-key errors ([`f8f804c`](https://github.com/jolars/panache/commit/f8f804c89aceabced19e6063932f4621882b3f61))

## [0.15.0](https://github.com/jolars/panache/compare/panache-parser-v0.14.0...panache-parser-v0.15.0) (2026-06-07)

### Features
- **parser:** tokenize math operators into `MATH_OPERATOR` ([`303e05b`](https://github.com/jolars/panache/commit/303e05bdd245f08fdb7c2244df6c1df198faaea4))
- **parser:** parse math content into a structural CST ([`cfb0c45`](https://github.com/jolars/panache/commit/cfb0c45f5173b49a49853660d1f4030debedd26c))
- **parser:** add syntax-error channel for embedded YAML ([`523fb62`](https://github.com/jolars/panache/commit/523fb62306ab9e0749651b6e4103cf8e3510f9d2))
- **parser:** embed prefix-aware YAML under HASHPIPE_YAML_CONTENT ([`d515896`](https://github.com/jolars/panache/commit/d515896f6da9ad307015c69c5348ee1d077d7b2a))
- **parser:** prefix-aware YAML scanner and builder ([`66a8e99`](https://github.com/jolars/panache/commit/66a8e99bdcb6c2b803bfa9ce227b132031006f3d))
- **parser:** drop host envelope from standalone YAML parse ([`5fecc99`](https://github.com/jolars/panache/commit/5fecc99a61e8429c97764f0a34590ef5d28c223f))
- inline YAML parser CST into Panache's CST ([`d240130`](https://github.com/jolars/panache/commit/d240130a59cc8018227d7d4fe71fae9b39ea0947))
- **parser:** swap YAML parser to our built-in parser ([`4ed243a`](https://github.com/jolars/panache/commit/4ed243ab2c8d9d9d5a0bc404ffaccf44c9b28ea7))
- **extensions:** add `space_reference_links` extension ([`309739d`](https://github.com/jolars/panache/commit/309739dbaf54dc84d588fbc45285bcb96795177e))
- **extensions:** add `wikilinks_title_after/before_pipe` ([`49500f1`](https://github.com/jolars/panache/commit/49500f12b27851789942b18b13db68d4fd691726))

### Bug Fixes
- **parser:** reject bare multi-word fence info in Pandoc ([`395b000`](https://github.com/jolars/panache/commit/395b0008c0a46f7a490d982f793f2dbe0f3a7737))
- **parser:** peel line prefix after a hashpipe block scalar ([`e246d30`](https://github.com/jolars/panache/commit/e246d30ca7aedf6cc96707dd30a14247f4736760))
- **parser:** don't start list at continuation for footnote def ([`9494b14`](https://github.com/jolars/panache/commit/9494b143e69cdb8d02ab99ad88b087ff9970a8ee)), closes [#348](https://github.com/jolars/panache/issues/348)
- **parser:** detect grid borders on dispatch line inside list items ([`e7fa051`](https://github.com/jolars/panache/commit/e7fa05124e3b4c2d14aceb13ce154bda022270e4))
- **parser:** lift tables and fenced divs from list-item content ([`6f3821c`](https://github.com/jolars/panache/commit/6f3821c4d7bedbcd47d56ad0851eac015f05adcd))

### Performance Improvements
- **parser:** gate table detection before whole-buffer strip ([`f27fc80`](https://github.com/jolars/panache/commit/f27fc80f0e2b4fa7a351d35d0a7195e048ca666c))

## [0.14.0](https://github.com/jolars/panache/compare/panache-parser-v0.13.0...panache-parser-v0.14.0) (2026-06-02)

### Features
- **config:** abort on unknown extensions, add exts to schema ([`397e1e5`](https://github.com/jolars/panache/commit/397e1e58a83e42a1decfb7692114099702fe681d))
- **cli:** allow `-o extensions.<name>=<bool>` overrides ([`2df73ab`](https://github.com/jolars/panache/commit/2df73ab3153b1f4e009a930536f3f590d1a0ef37))
- **formatter:** add `east_asian_line_breaks` extension ([`4f28716`](https://github.com/jolars/panache/commit/4f2871673d2ba4d00142032d066386db151179e9)), in [#339](https://github.com/jolars/panache/issues/339), closes [#339](https://github.com/jolars/panache/issues/339)

### Bug Fixes
- **parser:** reject deeply-indented empty bullets as nested lists ([`15691ff`](https://github.com/jolars/panache/commit/15691ffdc2c2ad6c1180dbee12f540607f01f602)), ref [#341](https://github.com/jolars/panache/issues/341)
- **parser:** restrict bare-URI autolinks to known schemes (#337) ([`930db45`](https://github.com/jolars/panache/commit/930db45b8f7bf71f08e3bdb4f036e5a6928936d9)), closes [#336](https://github.com/jolars/panache/issues/336)
- **parser:** keep `.class`/`#id` on executable fence info ([`4c8f396`](https://github.com/jolars/panache/commit/4c8f39682b6de5c887f0727a39b0f18b264ec762)), fixes [#334](https://github.com/jolars/panache/issues/334)
## [0.13.0](https://github.com/jolars/panache/compare/panache-parser-v0.12.0...panache-parser-v0.13.0) (2026-05-29)

### Features
- **parser:** reject yaml node property under parent key indent ([`db371fd`](https://github.com/jolars/panache/commit/db371fd97830263f0410cfb59a0e9a9f4898319e))
- **parser:** reject yaml %YAML directive with malformed version ([`557b116`](https://github.com/jolars/panache/commit/557b1162f41d450280af35aa89cd488aedbd6b00))
- **parser:** reject invalid yaml block-scalar indent + tab-in-quoted ([`e577390`](https://github.com/jolars/panache/commit/e577390270b8c784e64ad67d0a3f8a4456034ebe))
- **parser:** detect tab-in-indent-slot in yaml `check_tab_as_indent` ([`8b2ece9`](https://github.com/jolars/panache/commit/8b2ece90325a4a860b5c9bd7c20b784c5bc6d690))
- **parser:** reject yaml anchor in invalid positions ([`c8b8d6d`](https://github.com/jolars/panache/commit/c8b8d6d311c3c7a5f5d9fa3f1d04089aaa1226ed))
- **parser:** reject yaml tag with c-flow-indicator char ([`d83dca9`](https://github.com/jolars/panache/commit/d83dca99f7379a08462a5cfe2cacf54551687183))
- **parser:** reject yaml anchor decorating alias node ([`53289ca`](https://github.com/jolars/panache/commit/53289cac2dcb7efb8bbe2260463e5a39dd8c9cdb))
- **parser:** dispatch yaml `!tag` tokens in scanner ([`378a380`](https://github.com/jolars/panache/commit/378a3803889ccf11920416b68a40e17ddc62707f))
- **parser:** wrap indentless yaml seq when anchor decorates value ([`a7ca3c1`](https://github.com/jolars/panache/commit/a7ca3c1902eb58db57fc7fb7905d8586f51cc515))
- **parser:** propagate yaml anchors and aliases through flow projection ([`5874f78`](https://github.com/jolars/panache/commit/5874f785cc7c40534c25c3add630ec45dbc9d03e))
- **parser:** dispatch yaml `&anchor` / `*alias` in scanner ([`3959305`](https://github.com/jolars/panache/commit/39593053eecff1fa9bc293f0e2e3aaa27cb1aa53))
- **parser:** support flow collections as complex yaml keys ([`f1799d2`](https://github.com/jolars/panache/commit/f1799d23a15f850931b756b265ba7a574cf83e92))
- **parser:** reject yaml doc markers in flow and seq-item quoted dedent ([`8eddfdc`](https://github.com/jolars/panache/commit/8eddfdc40446c1902ab59e2b0ef3d8f8e5f20471))
- **parser:** reject doc-level comment-split plain scalar (BS4K) ([`641313a`](https://github.com/jolars/panache/commit/641313a60ec92844163567b87b2a7c4f3f7b8857))

### Bug Fixes
- add `inline-images` to gfm flavor ([`8ade630`](https://github.com/jolars/panache/commit/8ade63092ef9dc58bab04d37a2f9fa44a7256d0f))
- **parser:** preserve `\<ws>` escape arg and tab-as-content in yaml fold ([`c99c6a5`](https://github.com/jolars/panache/commit/c99c6a509ded4420b1bdb01030aaf7f87ca3f25c))
- **parser:** emit yaml anchor before tag in event projection ([`26c0b5f`](https://github.com/jolars/panache/commit/26c0b5fc98feccef606f28ee49aade9f3a90375a))
- **parser:** allow column-0 block scalar body at doc root ([`a0f358c`](https://github.com/jolars/panache/commit/a0f358c47a3635d050adbdc96810e8fccab1c37d))
- **parser:** reject YAML comment not preceded by space ([`a6125c3`](https://github.com/jolars/panache/commit/a6125c361b0b86ebac4a4bc76237f59aee9cc1ca))
- keep grid tables at column 0 to match pandoc ([`73016e3`](https://github.com/jolars/panache/commit/73016e3acabdfff0b0c800e8c557ea51a63456b4))
- **parser:** reject unterminated and over-indented YAML scalars ([`23f855e`](https://github.com/jolars/panache/commit/23f855ebfa2b14c1a908d031aef464cdc0bb155a))
## [0.12.0](https://github.com/jolars/panache/compare/panache-parser-v0.11.0...panache-parser-v0.12.0) (2026-05-26)

### Features
- **extensions:** support `four-space-rule` extension ([`77768ba`](https://github.com/jolars/panache/commit/77768bab3daec6dbae3a8d1d629add0d4b0700c8)), closes [#308](https://github.com/jolars/panache/issues/308)

### Bug Fixes
- **parser:** walk chars in `advance_columns` ([`c0f983b`](https://github.com/jolars/panache/commit/c0f983ba30bfb899605b5b0ca1b2acff9d2df915)), closes [#314](https://github.com/jolars/panache/issues/314), [#315](https://github.com/jolars/panache/issues/315), [#316](https://github.com/jolars/panache/issues/316), [#317](https://github.com/jolars/panache/issues/317), [#318](https://github.com/jolars/panache/issues/318), [#319](https://github.com/jolars/panache/issues/319), [#320](https://github.com/jolars/panache/issues/320), [#321](https://github.com/jolars/panache/issues/321), and [#322](https://github.com/jolars/panache/issues/322)
- **parser:** parse blockquotes flush against div fences ([`faf7ad1`](https://github.com/jolars/panache/commit/faf7ad12544f1d3e175edbd73d1fae1d017a0395)), closes [#310](https://github.com/jolars/panache/issues/310) and [#309](https://github.com/jolars/panache/issues/309)
- **formatter:** normalize smart dashes in headings, guard rule ([`82c9a31`](https://github.com/jolars/panache/commit/82c9a310fc3f88be88b68101e45bcbaa2f7b425c))
- **parser:** enable reference links in GFM defaults ([`581ebfb`](https://github.com/jolars/panache/commit/581ebfb5c493ec62db00d61a8661f602c9d3b300))
- **parser:** parse multiline tables in list+blockquote ([`74896c6`](https://github.com/jolars/panache/commit/74896c623cb23edfb5ce5b5d5b5170665141d922))
- **parser:** recognize nested grid/simple tables ([`feb5693`](https://github.com/jolars/panache/commit/feb5693501dde57596663dd90da28bc872cac1be))
- **parser:** detect pipe tables in list+blockquote ([`75a3157`](https://github.com/jolars/panache/commit/75a3157cda831b70a99c74588455abc0d902d3fa))
## [0.11.0](https://github.com/jolars/panache/compare/panache-parser-v0.10.0...panache-parser-v0.11.0) (2026-05-20)

### Features
- add JSON schema for configuration ([`5ae80bf`](https://github.com/jolars/panache/commit/5ae80bf1ebb75c2e41b2cf8115f301406af10816)), closes [#295](https://github.com/jolars/panache/issues/295)

### Bug Fixes
- **parser:** strip list+bq prefix on line-block lookahead ([`280c6c1`](https://github.com/jolars/panache/commit/280c6c1774ab2b226c0018fcdc96bb03b4449643))
- **parser:** use stripped content in def-list emit ([`a8ba276`](https://github.com/jolars/panache/commit/a8ba276990a2f73951017869c9846f6ed74299be))
- **parser:** strip list+bq prefix on fenced-code lookahead ([`bc0efc3`](https://github.com/jolars/panache/commit/bc0efc35168cd2b70bf54a50841e598fc37b6b1c))
- **parser:** emit `BLOCK_QUOTE_MARKER` for bq continuations in footnotes ([`f24b787`](https://github.com/jolars/panache/commit/f24b787f28e4cff6307f739daf400cadfe8cf0af))
- **parser:** dispatch bq-in-listitem first-line HTML blocks ([`bc32e49`](https://github.com/jolars/panache/commit/bc32e492b9ea09f6ffe37b3aa23ba330ed632a5c))
- **parser:** dispatch bq-in-listitem first-line content ([`c1c0db5`](https://github.com/jolars/panache/commit/c1c0db50358dc02ae1ec6efe6f000e99eea89e35))
- interpret a-j alphabetical list as one list ([`bed78dd`](https://github.com/jolars/panache/commit/bed78dd0b42bd9dde99c60a2cc08be31b0f99507))
## [0.10.0](https://github.com/jolars/panache/compare/panache-parser-v0.9.0...panache-parser-v0.10.0) (2026-05-17)

### Features
- **linter:** add `heading-eaten-attrs` + `heading-strip-comments-residue` ([`966135d`](https://github.com/jolars/panache/commit/966135da659ecf8be64127c34dd26649941d958f)), closes [#288](https://github.com/jolars/panache/issues/288)

### Bug Fixes
- **parser:** let blockquotes close lists properly ([`88ca2c2`](https://github.com/jolars/panache/commit/88ca2c22bb7eecee8383282a4488b764009c00cd)), closes [#292](https://github.com/jolars/panache/issues/292)
- **parser:** treat footnote refs inside footnote-def bodies as text ([`1f37425`](https://github.com/jolars/panache/commit/1f37425d4d4007594ad43b54b05837e72702499e)), ref [#290](https://github.com/jolars/panache/issues/290)
- **parser:** lift bq + multi-line `<div>` open + same-line close ([`259241a`](https://github.com/jolars/panache/commit/259241a95794ec18165a53c4290a98d629a4b415))
- **parser:** lift multi-line `<div>` open + same-line close ([`61e1df1`](https://github.com/jolars/panache/commit/61e1df126ff0e1c6462ed420d874c8fad688acff))
- **parser:** widen `<div>` lift for depth-aware and unclosed shapes ([`c7e4830`](https://github.com/jolars/panache/commit/c7e483040224f355235d325e57147e13f468cddc))
- **parser:** handle `:`-captions directly before `:::` ([`2f6a3ca`](https://github.com/jolars/panache/commit/2f6a3ca8c1c239101eddf409342e8dc6659d1fd6))
- **parser:** lift same-line HTML block with trailing text ([`add805e`](https://github.com/jolars/panache/commit/add805e75b3845291cfe3a53df342ee68cd2a20c))
- **parser:** lift list-item Comment/PI trailing-text split ([`50b4b45`](https://github.com/jolars/panache/commit/50b4b45db76bbab613322fb8fb71e8ae3ceefa66))
- **parser:** demote indented isInlineTag to RawInline ([`c0cf92b`](https://github.com/jolars/panache/commit/c0cf92bb36876c433bd72968457453f15d77b5be))
- **projector:** strip RawBlock first-line indent ([`926096e`](https://github.com/jolars/panache/commit/926096e9e7e1ce23b0c4de5b2de07ab125d1d1b3))
- **parser:** bq-wrapped HTML comment/PI trailing split ([`af26bdd`](https://github.com/jolars/panache/commit/af26bdd9fa741d403da1596aa68b5651c4f8ddad))
- **parser:** split Pandoc HTML comment / PI trailing-text ([`3171eae`](https://github.com/jolars/panache/commit/3171eae255db17ce1cc0ae5e106b9d6f6689393a))
- **parser:** strip list-item indent for HTML-block lift ([`f19ec57`](https://github.com/jolars/panache/commit/f19ec57d3c074308d4160164c32fda0550e45116))
- **parser:** lift multi-line HTML blocks as list-item ([`faf5c85`](https://github.com/jolars/panache/commit/faf5c851d82f56022e9b8ce19683fffb17c0cb79))
- **parser:** lift same-line HTML block as sole list-item content ([`cb0a2c1`](https://github.com/jolars/panache/commit/cb0a2c1bc707b49a837ce20202eb6b4b59b6b76f))
- **parser:** route indented HTML close-tag bytes ([`82bc43d`](https://github.com/jolars/panache/commit/82bc43d54d10ac743c42a797c5f988229ff1af56))
- **parser:** keep HTML_BLOCK on standalone </div> close form ([`fe1cd9c`](https://github.com/jolars/panache/commit/fe1cd9c7bc4728bf1549da3037b15abe087d0fe6))
- **parser:** lift mutliline html tags with trailing bytes ([`ea463f3`](https://github.com/jolars/panache/commit/ea463f34fc935746a825ec8119433c37e96496cf))
- **parser:** structurally lift multi-line HTML opens ([`5d65a02`](https://github.com/jolars/panache/commit/5d65a02d996b350dd4b36b8eeb744228e828a5e0))
- **parser:** avoid HTML_BLOCK_DIV panic on multi-line div ([`5613174`](https://github.com/jolars/panache/commit/561317490a03a2ef439e51481273397515d6c179))
## [0.9.0](https://github.com/jolars/panache/compare/panache-parser-v0.8.0...panache-parser-v0.9.0) (2026-05-12)

### Features
- **parser:** handle multi-line div tag blocks ([`5f350b4`](https://github.com/jolars/panache/commit/5f350b42111bcea7636c8a7283bc1c4fbe32c40e))

### Bug Fixes
- **parser:** lift bq messy-shape HTML bodies into CST ([`e923d7c`](https://github.com/jolars/panache/commit/e923d7c4ee8ca936a5a9d34a8b9190c35a28d7c9))
- **parser:** lift bq same-line HTML body into CST ([`1ba1b1e`](https://github.com/jolars/panache/commit/1ba1b1ea37dcdf7ecea15ecdf3ad7bb31af9ff33))
- **parser:** expose HTML_ATTRS for non-div strict-block tags in bq ([`2bd4542`](https://github.com/jolars/panache/commit/2bd4542bb8c7144523c6ec9894584b3038670315))
- **parser:** extend bq HTML lift to non-div and inline-block ([`8b88578`](https://github.com/jolars/panache/commit/8b8857897dd972b34aaacec47caa29477b155ed6))
- **parser:** lift bq-wrapped clean `<div>` body into CST ([`4bc4612`](https://github.com/jolars/panache/commit/4bc4612c08347607c605971e852fd3199dc850e6))
- **parser:** lift matched-pair inline-block HTML bodies into CST ([`f335b42`](https://github.com/jolars/panache/commit/f335b4218f39a99ba185ec27e0296ab67dc1bcad)), fix [#4](https://github.com/jolars/panache/issues/4)
- **parser:** lift multi-line non-div strict-block HTML opens into CST ([`59a5f91`](https://github.com/jolars/panache/commit/59a5f91aa763ec29cd1ccfca03b753d8ff106fb0))
- **parser:** lift non-div strict-block butted-close shapes into CST ([`98767ab`](https://github.com/jolars/panache/commit/98767ab92f3376e2eae79634c80bdaa4d868fecf)), fix [#4](https://github.com/jolars/panache/issues/4)
- **parser:** lift inner strict-block HTML elements into CST ([`3f6f644`](https://github.com/jolars/panache/commit/3f6f6448cb87154f2b8cb363a747fb50cc496a95))
- **projector:** lift empty `<div>` into structural CST walk ([`179a681`](https://github.com/jolars/panache/commit/179a681b12eedc54704d5e42826e36a0d8812ebf)), fix [#4](https://github.com/jolars/panache/issues/4)
- **projector:** strip blockquote markers from HTML block bodies ([`47e6c38`](https://github.com/jolars/panache/commit/47e6c386527daff8dff4ca30fed708ff2c762418))
- **parser:** lift same-line `<div>` shapes into CST ([`33b6297`](https://github.com/jolars/panache/commit/33b6297ffae9711a8459d1f0e0e60b2a2a2926c5))
- **parser:** lift messy `<div>` shapes into CST ([`4c03405`](https://github.com/jolars/panache/commit/4c034054f52275e33903e9b3f066e7fdf175743a))
- **parser:** lift inner `<div>` elements into CST ([`1b37801`](https://github.com/jolars/panache/commit/1b37801fc12e12dd57a239bc6a643527df640c27))
- **parser:** mirror Pandoc's `isInlineTag` for `<script>` ([`ba9c96f`](https://github.com/jolars/panache/commit/ba9c96f39e338300dac97347ea0bb8583e813a66))
- **parser,formatter:** don't escape `[`, `]` ([`26bbb1c`](https://github.com/jolars/panache/commit/26bbb1c5bd539c85108f63e79dbe7c29d24b5222))
- **parser:** capture citation inside reference ([`c6685f4`](https://github.com/jolars/panache/commit/c6685f48d886d014831e83a30c71593a5692687e)), closes [#278](https://github.com/jolars/panache/issues/278)
- **parser:** correctly merge unevenly indented lists ([`b661b61`](https://github.com/jolars/panache/commit/b661b61a50a72d302713e0fd5a50d3a1ab66e87f)), fixes [#277](https://github.com/jolars/panache/issues/277)
- **parser:** closer cannot interrupt under pandoc ([`74d333a`](https://github.com/jolars/panache/commit/74d333a0e473cfda655a92104584afb6a1df9f17))
- **parser:** don't let `<style>` tags interrupt under pandoc ([`b77db95`](https://github.com/jolars/panache/commit/b77db958480be7e049232860d6df10a961c980ce))
- **parser:** fix plain/paragraph handling for html in parser ([`d7745dd`](https://github.com/jolars/panache/commit/d7745ddcb720f8464225c16397c1c3ba4c51889f))
- **parser:** accept correct tags for Pandoc's closing-forms ([`7ab94d1`](https://github.com/jolars/panache/commit/7ab94d183cb794362acbe84f63eb6278063d8454))
- **parser:** match Pandoc on closing forms of inline blocks ([`525cdf4`](https://github.com/jolars/panache/commit/525cdf40b22e56d2cbcfd6c6bce146a1874c453d))
- **parser:** handle multi-line void open tag ([`05b369d`](https://github.com/jolars/panache/commit/05b369d072d2d243f59261b955c67672079561d5))
- **parser:** handle infinite recursion in incomplete tags ([`95c95bf`](https://github.com/jolars/panache/commit/95c95bfe918d786142bc18f2290c301518fe15c9))
- **parser:** handle Pandoc's void block tags ([`a327162`](https://github.com/jolars/panache/commit/a32716225851593bb1caa9308f24112ab18c660a))
- **parser:** handle context-aware block/inline dispatcher ([`1b8330d`](https://github.com/jolars/panache/commit/1b8330da6017c53a83ab460af4e9ecefeedcba96))
- **parser:** don't hardcode `<div` into CST ([`7c6515e`](https://github.com/jolars/panache/commit/7c6515e058b5df4eec014b2d1c604674d025d846))
- **parser:** fix dialect-divergence in pandoc/commonmark ([`3a81ac2`](https://github.com/jolars/panache/commit/3a81ac245dc758d41ce0682c8bab01e52b04f54d))
## [0.8.0](https://github.com/jolars/panache/compare/panache-parser-v0.7.1...panache-parser-v0.8.0) (2026-05-09)

### Features
- **parser:** add depth-aware html block parsing ([`2a5dcac`](https://github.com/jolars/panache/commit/2a5dcace3361acb49c222b5bdcf3ef28d3dd8e8b))
- **cli:** add a `--to pandoc-json` argument ([`b3f3785`](https://github.com/jolars/panache/commit/b3f378558ef9dab11beb15c6e2ff85cfdbffec28)), closes [#269](https://github.com/jolars/panache/issues/269)
- **parser:** gate html declarations on dialect ([`9e0b645`](https://github.com/jolars/panache/commit/9e0b64561f39ebf7856263058947a27c7022dde8))
- **parser:** parser inline spans granularly ([`03333d2`](https://github.com/jolars/panache/commit/03333d241000a0cbea6648967bf08fd940b4e0ab))

### Bug Fixes
- correctly parser trailing attributes in equations ([`492306f`](https://github.com/jolars/panache/commit/492306f2cdaa35ef64b6e43b914797555f5681d9))
- **parser:** parse references in captions ([`eb29a9d`](https://github.com/jolars/panache/commit/eb29a9d1dfb44c6d9626570e2015eb7898ca166e))
- **parser:** add commonmark-ascii fix ([`4cfcd1c`](https://github.com/jolars/panache/commit/4cfcd1cdcc4575906faffc21b86fa1f7f52a5cb9))
- **parser,linter:** introduce `HTML_DIV_BLOCK` parsing ([`3962e03`](https://github.com/jolars/panache/commit/3962e0329a83feb5bfbdef84fd3bf52527e7af58)), closes [#263](https://github.com/jolars/panache/issues/263)
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
