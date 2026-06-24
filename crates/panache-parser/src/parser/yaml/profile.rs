//! Consumer profiles for YAML validation.
//!
//! Panache's YAML structural validator (`super::validator`) mirrors abstract
//! **YAML 1.2** — the contract the vendored yaml-test-suite holds it to. But
//! the *real* consumers of the YAML in a document are stricter and differ by
//! context:
//!
//! - **Frontmatter** is read by **pandoc** → Haskell `yaml`/libyaml (≈ 1.1).
//! - **Hashpipe `#|` cell options** are read by **quarto** → js-yaml (1.2).
//! - In a **Quarto** doc, quarto parses the frontmatter (js-yaml) *and* hands
//!   the document to pandoc, which re-parses the metadata (libyaml). So Quarto
//!   frontmatter must satisfy **both** — the stricter wins.
//!
//! A [`YamlValidationContext`] captures which real consumers apply to a given
//! (flavor, location), so the validator can layer consumer-only checks on top
//! of the 1.2 substrate. The empirical basis for each consumer's accept/reject
//! behavior is the oracle audit in `scripts/yaml-oracle/` and its classified
//! output in `crates/panache-parser/tests/yaml/consumer-matrix.md`.
//!
//! The substrate path ([`YamlValidationContext::substrate`]) runs every 1.2
//! check and **no** consumer-only checks — it is what the yaml-test-suite tests
//! exercise, so its verdicts never change.

use crate::options::Flavor;

/// A real-world YAML parser whose accept/reject behavior Panache mirrors. These
/// are distinct measured consumers, not interchangeable libyaml wrappers — see
/// `scripts/yaml-oracle/oracle.json` and `tests/yaml/consumer-matrix.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum YamlConsumer {
    /// pandoc's Haskell `yaml` (libyaml, ≈ YAML 1.1). Reads frontmatter. The
    /// lenient baseline: accepts duplicate keys (last value wins) and tabs in
    /// some positions.
    Libyaml,
    /// js-yaml (YAML 1.2). Reads Quarto frontmatter and hashpipe `#|` options.
    /// Rejects duplicate keys and tabs.
    Jsyaml,
    /// R's `yaml` package (libyaml-based), used by the RMarkdown toolchain —
    /// `rmarkdown::yaml_front_matter` for frontmatter and knitr for `#|` chunk
    /// options. Like libyaml but, measured against the suite, additionally
    /// REJECTS duplicate keys and tabs.
    RYaml,
}

/// A small bitset over [`YamlConsumer`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ConsumerSet(u8);

impl ConsumerSet {
    const fn bit(consumer: YamlConsumer) -> u8 {
        match consumer {
            YamlConsumer::Libyaml => 0b001,
            YamlConsumer::Jsyaml => 0b010,
            YamlConsumer::RYaml => 0b100,
        }
    }

    /// Every consumer — the rejection set for a check all real parsers make.
    pub const fn all() -> Self {
        ConsumerSet(0b111)
    }

    /// The empty set — no real consumer applies (lenient).
    pub const fn empty() -> Self {
        ConsumerSet(0)
    }

    /// A single-consumer set.
    pub const fn of(consumer: YamlConsumer) -> Self {
        ConsumerSet(Self::bit(consumer))
    }

    /// Add a consumer.
    pub const fn with(self, consumer: YamlConsumer) -> Self {
        ConsumerSet(self.0 | Self::bit(consumer))
    }

    /// Does this set contain `consumer`?
    pub const fn contains(self, consumer: YamlConsumer) -> bool {
        self.0 & Self::bit(consumer) != 0
    }

    /// Do the two sets share any consumer?
    pub const fn intersects(self, other: ConsumerSet) -> bool {
        self.0 & other.0 != 0
    }

    /// Is this set empty?
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

/// Where in a document the YAML lives. Frontmatter is read by pandoc (and, in
/// Quarto, also js-yaml); hashpipe `#|` cell options are read by the executable
/// engine (js-yaml for Quarto, the R `yaml` package for RMarkdown).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum YamlLocation {
    Frontmatter,
    Hashpipe,
}

/// The consumers that must accept a YAML region, derived from (flavor,
/// location). Validation rejects a region iff **any** active consumer rejects
/// it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct YamlValidationContext {
    consumers: ConsumerSet,
    substrate: bool,
}

impl YamlValidationContext {
    /// The abstract YAML-1.2 substrate: run every 1.2 check and no consumer-only
    /// checks. This is the contract the yaml-test-suite holds the validator to,
    /// so its verdicts are independent of any flavor/location.
    pub const fn substrate() -> Self {
        Self {
            consumers: ConsumerSet::empty(),
            substrate: true,
        }
    }

    /// Build a production context for a (flavor, location).
    pub fn new(flavor: Flavor, location: YamlLocation) -> Self {
        let consumers = match location {
            YamlLocation::Frontmatter => frontmatter_consumers(flavor),
            YamlLocation::Hashpipe => hashpipe_consumers(flavor),
        };
        Self {
            consumers,
            substrate: false,
        }
    }

    /// Convenience: a frontmatter context for `flavor`.
    pub fn frontmatter(flavor: Flavor) -> Self {
        Self::new(flavor, YamlLocation::Frontmatter)
    }

    /// Convenience: a hashpipe `#|` context for `flavor`.
    pub fn hashpipe(flavor: Flavor) -> Self {
        Self::new(flavor, YamlLocation::Hashpipe)
    }

    /// True for the 1.2 substrate path (suite tests). Consumer-only checks must
    /// never run here.
    pub const fn is_substrate(&self) -> bool {
        self.substrate
    }

    /// The active consumer set.
    pub const fn consumers(&self) -> ConsumerSet {
        self.consumers
    }

    /// True when at least one active consumer is among `rejecting` — i.e. a
    /// consumer-only check whose rejection set is `rejecting` should fire.
    pub const fn any_rejects(&self, rejecting: ConsumerSet) -> bool {
        self.consumers.intersects(rejecting)
    }
}

/// Frontmatter consumers by flavor:
/// - Pandoc: pandoc/libyaml only.
/// - Quarto: quarto parses the frontmatter (js-yaml) and then hands the doc to
///   pandoc (libyaml), so both must accept.
/// - RMarkdown: `rmarkdown::yaml_front_matter` (R `yaml`) reads it, then the doc
///   renders through pandoc (libyaml), so both must accept.
/// - GFM/CommonMark/MultiMarkdown: no asserted YAML metadata consumer — lenient.
///
/// See `tests/yaml/consumer-matrix.md`.
fn frontmatter_consumers(flavor: Flavor) -> ConsumerSet {
    match flavor {
        Flavor::Pandoc => ConsumerSet::of(YamlConsumer::Libyaml),
        Flavor::Quarto => ConsumerSet::of(YamlConsumer::Libyaml).with(YamlConsumer::Jsyaml),
        Flavor::RMarkdown => ConsumerSet::of(YamlConsumer::Libyaml).with(YamlConsumer::RYaml),
        Flavor::Gfm | Flavor::CommonMark | Flavor::MultiMarkdown | Flavor::Mdsvex => {
            ConsumerSet::empty()
        }
    }
}

/// Hashpipe `#|` cell options are parsed by the executable engine: js-yaml for
/// Quarto, the R `yaml` package for RMarkdown (via knitr). Other flavors do not
/// recognize executable cells, so no hashpipe region reaches validation there.
fn hashpipe_consumers(flavor: Flavor) -> ConsumerSet {
    match flavor {
        Flavor::Quarto => ConsumerSet::of(YamlConsumer::Jsyaml),
        Flavor::RMarkdown => ConsumerSet::of(YamlConsumer::RYaml),
        Flavor::Pandoc
        | Flavor::Gfm
        | Flavor::CommonMark
        | Flavor::MultiMarkdown
        | Flavor::Mdsvex => ConsumerSet::empty(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn substrate_runs_no_consumer_checks() {
        let ctx = YamlValidationContext::substrate();
        assert!(ctx.is_substrate());
        assert!(ctx.consumers().is_empty());
    }

    #[test]
    fn pandoc_frontmatter_is_libyaml_only() {
        let ctx = YamlValidationContext::frontmatter(Flavor::Pandoc);
        assert!(!ctx.is_substrate());
        assert!(ctx.consumers().contains(YamlConsumer::Libyaml));
        assert!(!ctx.consumers().contains(YamlConsumer::Jsyaml));
    }

    #[test]
    fn quarto_frontmatter_is_both() {
        let ctx = YamlValidationContext::frontmatter(Flavor::Quarto);
        assert!(ctx.consumers().contains(YamlConsumer::Libyaml));
        assert!(ctx.consumers().contains(YamlConsumer::Jsyaml));
    }

    #[test]
    fn quarto_hashpipe_is_jsyaml_only() {
        let ctx = YamlValidationContext::hashpipe(Flavor::Quarto);
        assert!(ctx.consumers().contains(YamlConsumer::Jsyaml));
        assert!(!ctx.consumers().contains(YamlConsumer::Libyaml));
    }

    #[test]
    fn rmarkdown_uses_pandoc_and_r_yaml() {
        let fm = YamlValidationContext::frontmatter(Flavor::RMarkdown);
        assert!(fm.consumers().contains(YamlConsumer::Libyaml)); // renders via pandoc
        assert!(fm.consumers().contains(YamlConsumer::RYaml)); // rmarkdown::yaml_front_matter
        assert!(!fm.consumers().contains(YamlConsumer::Jsyaml));

        let hp = YamlValidationContext::hashpipe(Flavor::RMarkdown);
        assert!(hp.consumers().contains(YamlConsumer::RYaml)); // knitr
        assert!(!hp.consumers().contains(YamlConsumer::Jsyaml));
        assert!(!hp.consumers().contains(YamlConsumer::Libyaml));
    }

    #[test]
    fn commonmark_frontmatter_is_lenient() {
        let ctx = YamlValidationContext::frontmatter(Flavor::CommonMark);
        assert!(ctx.consumers().is_empty());
        assert!(!ctx.is_substrate());
    }

    #[test]
    fn any_rejects_matches_intersection() {
        // implicit-empty-key rejects under every consumer.
        let all = ConsumerSet::all();
        assert!(YamlValidationContext::frontmatter(Flavor::Pandoc).any_rejects(all));
        assert!(YamlValidationContext::frontmatter(Flavor::RMarkdown).any_rejects(all));

        // duplicate-key rejects under js-yaml (Quarto) and R yaml (RMarkdown),
        // not under pandoc/libyaml.
        let dup = ConsumerSet::of(YamlConsumer::Jsyaml).with(YamlConsumer::RYaml);
        assert!(!YamlValidationContext::frontmatter(Flavor::Pandoc).any_rejects(dup));
        assert!(YamlValidationContext::frontmatter(Flavor::Quarto).any_rejects(dup));
        assert!(YamlValidationContext::frontmatter(Flavor::RMarkdown).any_rejects(dup));
        assert!(YamlValidationContext::hashpipe(Flavor::RMarkdown).any_rejects(dup));
    }
}
