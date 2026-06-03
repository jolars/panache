//! Helpers for copying rowan green-tree fragments into an in-progress
//! `GreenNodeBuilder`. Used when one parser embeds another's CST subtree
//! (e.g. the YAML metadata block parser splices the `YAML_DOCUMENT`(s)
//! produced by the in-tree YAML parser into the host document CST).

use rowan::GreenNodeBuilder;

/// Recursively copy `node` (and all descendants) into `builder` as a
/// fresh subtree. The caller is responsible for any surrounding
/// `start_node` / `finish_node` framing.
pub(crate) fn copy_green_node(builder: &mut GreenNodeBuilder<'_>, node: &rowan::GreenNodeData) {
    builder.start_node(node.kind());
    copy_green_children(builder, node);
    builder.finish_node();
}

/// Copy the *children* of `node` into `builder` — but not `node` itself.
/// Useful when the outer container is a YAML-spec wrapper (like
/// `YAML_STREAM`) whose role is already played by an enclosing host
/// node (`YAML_METADATA_CONTENT`).
pub(crate) fn copy_green_children(builder: &mut GreenNodeBuilder<'_>, node: &rowan::GreenNodeData) {
    for child in node.children() {
        match child {
            rowan::NodeOrToken::Node(n) => copy_green_node(builder, n),
            rowan::NodeOrToken::Token(t) => builder.token(t.kind(), t.text()),
        }
    }
}
