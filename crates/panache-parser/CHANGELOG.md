# Changelog

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
