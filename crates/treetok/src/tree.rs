//! Minimal fork of `termtree 0.4.1` with an added [`Tree::render`] method.
//!
//! The standard [`std::fmt::Display`] impl is preserved unchanged.  The new
//! `render` method additionally passes the **full prefix string** (e.g.
//! `"│   ├── "`) to a caller-supplied closure so that depth-aware formatting
//! — such as column-aligned token counts — becomes possible.

use std::collections::VecDeque;
use std::fmt::{self, Display};
use std::io::{self, Write};
use std::rc::Rc;

/// A simple recursive type that renders its components in a tree-like format.
#[derive(Debug, Clone)]
pub struct Tree<D: Display> {
    /// The label for this node.
    pub root: D,
    /// Child nodes.
    pub leaves: Vec<Tree<D>>,
    multiline: bool,
    glyphs: GlyphPalette,
}

impl<D: Display> Tree<D> {
    /// Create a new tree node with no children.
    pub fn new(root: D) -> Self {
        Tree {
            root,
            leaves: Vec::new(),
            multiline: false,
            glyphs: GlyphPalette::new(),
        }
    }

    /// Builder: set the initial child nodes.
    pub fn with_leaves(mut self, leaves: impl IntoIterator<Item = impl Into<Tree<D>>>) -> Self {
        self.leaves = leaves.into_iter().map(Into::into).collect();
        self
    }

    /// Builder: ensure all lines for `root` are indented.
    pub fn with_multiline(mut self, yes: bool) -> Self {
        self.multiline = yes;
        self
    }

    /// Builder: customise the rendering glyphs for this node.
    pub fn with_glyphs(mut self, glyphs: GlyphPalette) -> Self {
        self.glyphs = glyphs;
        self
    }

    /// Ensure all lines for `root` are indented (mutable variant).
    pub fn set_multiline(&mut self, yes: bool) -> &mut Self {
        self.multiline = yes;
        self
    }

    /// Customise the rendering glyphs for this node (mutable variant).
    pub fn set_glyphs(&mut self, glyphs: GlyphPalette) -> &mut Self {
        self.glyphs = glyphs;
        self
    }

    /// Append a child node.
    pub fn push(&mut self, leaf: impl Into<Tree<D>>) -> &mut Self {
        self.leaves.push(leaf.into());
        self
    }

    /// Render the tree to `out`, calling `fmt_leaf(out, prefix_width, root)`
    /// for every node (root receives `prefix_width = 0`).
    ///
    /// `prefix_width` is the number of display columns already written by
    /// `render` before the closure is called — i.e. the width of the
    /// connector string (`"├── "`, `"│   ├── "`, …).  Each nesting level
    /// contributes exactly 4 columns, so `prefix_width` is always a multiple
    /// of 4.  Callers can use it directly to compute padding for column
    /// alignment without any string measuring.
    pub fn render<W, F>(&self, out: &mut W, fmt_leaf: &F) -> io::Result<()>
    where
        W: Write + ?Sized,
        F: Fn(&mut W, usize, &D) -> io::Result<()>,
    {
        fmt_leaf(out, 0, &self.root)?;
        writeln!(out)?;

        let mut queue: RenderQueue<D> = VecDeque::new();
        enqueue_leaves(&mut queue, self, Rc::new(Vec::new()));

        while let Some((last, leaf, spaces)) = queue.pop_front() {
            let prefix_width = (spaces.len() + 1) * 4;

            // render owns the prefix; the closure owns only the node content.
            for &ancestor_was_last in spaces.as_slice() {
                if ancestor_was_last {
                    write!(out, "{}{}", leaf.glyphs.last_skip, leaf.glyphs.skip_indent)?;
                } else {
                    write!(out, "{}{}", leaf.glyphs.middle_skip, leaf.glyphs.skip_indent)?;
                }
            }
            let connector = if last { leaf.glyphs.last_item } else { leaf.glyphs.middle_item };
            write!(out, "{}{}", connector, leaf.glyphs.item_indent)?;

            fmt_leaf(out, prefix_width, &leaf.root)?;
            writeln!(out)?;

            if !leaf.leaves.is_empty() {
                let mut child_spaces = spaces.as_ref().clone();
                child_spaces.push(last);
                enqueue_leaves(&mut queue, leaf, Rc::new(child_spaces));
            }
        }
        Ok(())
    }
}

type RenderQueue<'t, D> = VecDeque<(bool, &'t Tree<D>, Rc<Vec<bool>>)>;

fn enqueue_leaves<'t, D: Display>(
    queue: &mut RenderQueue<'t, D>,
    parent: &'t Tree<D>,
    spaces: Rc<Vec<bool>>,
) {
    for (i, leaf) in parent.leaves.iter().rev().enumerate() {
        queue.push_front((i == 0, leaf, spaces.clone()));
    }
}

// ─── Standard Display impl (unchanged from termtree 0.4.1) ───────────────────

#[allow(clippy::branches_sharing_code)]
impl<D: Display> Display for Tree<D> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.root.fmt(f)?;
        writeln!(f)?;
        let mut queue: VecDeque<(bool, &Tree<D>, Rc<Vec<bool>>)> = VecDeque::new();
        let no_space = Rc::new(Vec::new());
        enqueue_leaves(&mut queue, self, no_space);
        while let Some((last, leaf, spaces)) = queue.pop_front() {
            let mut prefix = (
                if last { leaf.glyphs.last_item } else { leaf.glyphs.middle_item },
                leaf.glyphs.item_indent,
            );

            if leaf.multiline {
                let rest_prefix = (
                    if last { leaf.glyphs.last_skip } else { leaf.glyphs.middle_skip },
                    leaf.glyphs.skip_indent,
                );
                let root = if f.alternate() {
                    format!("{:#}", leaf.root)
                } else {
                    format!("{}", leaf.root)
                };
                for line in root.lines() {
                    for s in spaces.as_slice() {
                        if *s {
                            self.glyphs.last_skip.fmt(f)?;
                            self.glyphs.skip_indent.fmt(f)?;
                        } else {
                            self.glyphs.middle_skip.fmt(f)?;
                            self.glyphs.skip_indent.fmt(f)?;
                        }
                    }
                    prefix.0.fmt(f)?;
                    prefix.1.fmt(f)?;
                    line.fmt(f)?;
                    writeln!(f)?;
                    prefix = rest_prefix;
                }
            } else {
                for s in spaces.as_slice() {
                    if *s {
                        self.glyphs.last_skip.fmt(f)?;
                        self.glyphs.skip_indent.fmt(f)?;
                    } else {
                        self.glyphs.middle_skip.fmt(f)?;
                        self.glyphs.skip_indent.fmt(f)?;
                    }
                }
                prefix.0.fmt(f)?;
                prefix.1.fmt(f)?;
                leaf.root.fmt(f)?;
                writeln!(f)?;
            }

            if !leaf.leaves.is_empty() {
                let mut child_spaces = spaces.as_ref().clone();
                child_spaces.push(last);
                enqueue_leaves(&mut queue, leaf, Rc::new(child_spaces));
            }
        }
        Ok(())
    }
}

impl<D: Display> From<D> for Tree<D> {
    fn from(inner: D) -> Self {
        Self::new(inner)
    }
}

impl<D: Display> Extend<D> for Tree<D> {
    fn extend<T: IntoIterator<Item = D>>(&mut self, iter: T) {
        self.leaves.extend(iter.into_iter().map(Into::into));
    }
}

impl<D: Display> Extend<Tree<D>> for Tree<D> {
    fn extend<T: IntoIterator<Item = Tree<D>>>(&mut self, iter: T) {
        self.leaves.extend(iter);
    }
}

// ─── Glyph palette (identical to termtree 0.4.1) ─────────────────────────────

/// The set of Unicode box-drawing strings used when rendering the tree.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct GlyphPalette {
    /// Connector for non-last siblings (default `"├"`).
    pub middle_item: &'static str,
    /// Connector for the last sibling (default `"└"`).
    pub last_item: &'static str,
    /// Indent after the connector (default `"── "`).
    pub item_indent: &'static str,
    /// Vertical continuation for non-last ancestors (default `"│"`).
    pub middle_skip: &'static str,
    /// Blank continuation for last ancestors (default `" "`).
    pub last_skip: &'static str,
    /// Indent after the vertical continuation (default `"   "`).
    pub skip_indent: &'static str,
}

impl GlyphPalette {
    /// Create the default glyph palette.
    pub const fn new() -> Self {
        Self {
            middle_item: "├",
            last_item: "└",
            item_indent: "── ",
            middle_skip: "│",
            last_skip: " ",
            skip_indent: "   ",
        }
    }
}

impl Default for GlyphPalette {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use rstest::{fixture, rstest};

    use super::*;

    // ── Display impl (ported from termtree 0.4.1) ─────────────────────────
    // Each case has a structurally distinct tree and a multi-line expected
    // string, so named tests are clearer than #[case] rows here.

    #[test]
    fn display_root_only() {
        let tree = Tree::new("foo");
        assert_eq!(format!("{tree}"), "foo\n");
    }

    #[test]
    fn display_nested_single_child() {
        let tree = Tree::new("foo").with_leaves([Tree::new("bar").with_leaves(["baz"])]);
        assert_eq!(
            format!("{tree}"),
            "foo\n\
             └── bar\n\
             \x20   └── baz\n"
        );
    }

    #[test]
    fn display_multiple_siblings() {
        let tree = Tree::new("foo").with_leaves(["bar", "baz"]);
        assert_eq!(
            format!("{tree}"),
            "foo\n\
             ├── bar\n\
             └── baz\n"
        );
    }

    #[test]
    fn display_multiline_labels() {
        let tree = Tree::new("foo").with_leaves([
            Tree::new("hello\nworld").with_multiline(true),
            Tree::new("goodbye\nworld").with_multiline(true),
        ]);
        assert_eq!(
            format!("{tree}"),
            "foo\n\
             ├── hello\n\
             │   world\n\
             └── goodbye\n\
             \x20   world\n"
        );
    }

    // ── render() — prefix_width delivered to closure ──────────────────────

    /// Collects every `prefix_width` value passed to the render closure.
    #[fixture]
    fn widths() -> RefCell<Vec<usize>> {
        RefCell::new(Vec::new())
    }

    #[rstest]
    // Root alone → single call with width 0.
    #[case(Tree::new("root"), vec![0])]
    // Linear chain: root=0, child=4, grandchild=8.
    #[case(
        Tree::new("a").with_leaves([Tree::new("b").with_leaves(["c"])]),
        vec![0, 4, 8]
    )]
    // Three siblings all share the same depth → all width 4.
    #[case(Tree::new("root").with_leaves(["x", "y", "z"]), vec![0, 4, 4, 4])]
    fn render_prefix_widths(
        #[case] tree: Tree<&str>,
        #[case] expected: Vec<usize>,
        widths: RefCell<Vec<usize>>,
    ) {
        tree.render(&mut std::io::sink(), &|_, pw, _| {
            widths.borrow_mut().push(pw);
            Ok(())
        })
        .unwrap();
        assert_eq!(*widths.borrow(), expected);
    }
}
