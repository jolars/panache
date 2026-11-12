//! Protect verified Jupyter math before Markdown block/container dispatch.

use super::*;

impl Parser<'_> {
    fn jupyter_math_closing_line(
        &mut self,
        line: usize,
        prefix: &ContainerPrefix,
    ) -> Option<usize> {
        let gobble = self.containers.gobble_chain();
        match self.containers.stack.last_mut() {
            Some(Container::Paragraph { buffer, .. }) => {
                buffer.jupyter_math_closing_line(line, prefix, &self.lines[line..])
            }
            Some(Container::ListItem { buffer, .. }) => {
                buffer.jupyter_math_closing_line(line, prefix, &self.lines[line..], &gobble)
            }
            _ => None,
        }
    }

    fn clear_pending_jupyter_math(&mut self) {
        self.jupyter_math_end = None;
        match self.containers.stack.last_mut() {
            Some(Container::Paragraph {
                open_display_math,
                open_inline_math_envs,
                ..
            }) => {
                *open_display_math = None;
                open_inline_math_envs.clear();
            }
            Some(Container::ListItem { buffer, .. }) => buffer.clear_open_math(),
            _ => {}
        }
    }

    pub(super) fn try_append_jupyter_math_continuation(
        &mut self,
        line: &str,
    ) -> Option<LineDispatch> {
        if !self.config.extensions.tex_math_jupyter {
            return None;
        }
        let depth = self.containers.depth();
        let verified = self
            .jupyter_math_end
            .is_some_and(|(end, owner)| self.pos <= end && owner == depth);
        let pending = match self.containers.last() {
            Some(Container::Paragraph {
                open_display_math,
                open_inline_math_envs,
                ..
            }) => open_display_math.is_some() || !open_inline_math_envs.is_empty(),
            Some(Container::ListItem { buffer, .. }) => buffer.has_open_display_math(),
            _ => false,
        };
        if !verified && !pending {
            self.jupyter_math_end = None;
            return None;
        }

        // Strip only existing containers: a further `>` is a TeX relation,
        // and a list marker in the body is math content rather than a sibling.
        let prefix = ContainerPrefix::from_stack(&self.containers.stack, false, self.config);
        let (held, content) = prefix.split_pieces(line);
        if is_blank_line(content) {
            self.clear_pending_jupyter_math();
            return None;
        }
        if !verified {
            let Some(last) = self.jupyter_math_closing_line(self.pos, &prefix) else {
                // Currency and unmatched delimiters must leave every block
                // detector available, including ordered lists and HTML.
                self.clear_pending_jupyter_math();
                return None;
            };
            self.jupyter_math_end = Some((last, depth));
        }

        match self.containers.stack.last_mut() {
            Some(Container::Paragraph { buffer, .. }) => {
                for text in held.token_texts() {
                    buffer.push_indent(text);
                }
                paragraphs::append_paragraph_line(
                    &mut self.containers,
                    &mut self.builder,
                    content,
                    self.config,
                );
            }
            Some(Container::ListItem {
                buffer,
                marker_only,
                ..
            }) => {
                for text in held.token_texts() {
                    buffer.push_prefix(text);
                }
                buffer.push_text(content, self.config);
                *marker_only = false;
            }
            _ => return None,
        }
        let mut consumed = 1;
        if self.jupyter_math_end == Some((self.pos, depth)) {
            // Another opener on the closing line can hold the next line too.
            if let Some(last) = self.jupyter_math_closing_line(self.pos + 1, &prefix) {
                self.jupyter_math_end = Some((last, depth));
            } else {
                self.clear_pending_jupyter_math();
                // A math continuation has no blank line before it, so the
                // ordinary header gate also prevents folding held math.
                if !self.config.extensions.blank_before_header {
                    consumed += usize::from(self.try_fold_held_inline_into_setext(&prefix, 1));
                }
            }
        }
        Some(LineDispatch::consumed(consumed))
    }
}
