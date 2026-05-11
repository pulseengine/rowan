//! Regression test for cy-208: Stacked-Borrows violation in
//! `rowan::cursor::free` caused by `Box::from_raw(NodeData)` running while
//! a sibling iterator (`SyntaxNodeChildren`) holds an active retag covering
//! the same `NodeData` allocation.
//!
//! Repro mirrors the cyrs pattern (`crates/cyrs-hir/src/lower.rs`):
//! call `.children()` on a parent and iterate, dropping each child
//! `SyntaxNode` after touching it. Each drop calls `free()`, which under
//! the bug uses `Box::from_raw(node_data)` -- whose drop adds a
//! strongly-protected retag that conflicts with the parent's still-live
//! permission held by the iterator.
//!
//! Run with: `MIRIFLAGS=-Zmiri-strict-provenance cargo +nightly miri test --test miri_cursor_free_sb`

use rowan::cursor::SyntaxNode;
use rowan::{GreenNodeBuilder, SyntaxKind};

#[test]
fn cursor_free_during_sibling_iteration_is_sound() {
    // Build a tree with a parent containing several child composite nodes,
    // each with a token. Many siblings exercise repeated Drop->free()
    // calls during iteration.
    let mut b = GreenNodeBuilder::new();
    b.start_node(SyntaxKind(0)); // root
    for i in 0..32u32 {
        b.start_node(SyntaxKind(1)); // child composite
        let s = format!("c{:03}", i);
        b.token(SyntaxKind(2), &s);
        b.finish_node();
    }
    b.finish_node();
    let green = b.finish();

    let root = SyntaxNode::new_root(green);

    // Iterate children: each yielded SyntaxNode is dropped at the end of
    // each loop iteration (freeing its NodeData) while the iterator
    // still holds the parent and is about to walk to the next sibling.
    // Touching `kind()` and `text_range()` exercises the Green deref
    // paths so miri retags the NodeData fields.
    let mut count = 0usize;
    for child in root.children() {
        let _k = child.kind();
        let _r = child.text_range();
        // Also iterate this child's tokens via children_with_tokens to
        // trigger nested NodeData allocations and frees inside the
        // outer iteration.
        for elem in child.children_with_tokens() {
            let _ = elem.kind();
        }
        count += 1;
    }
    assert_eq!(count, 32);

    // Drop root explicitly (not strictly required, but exercises the
    // free path with a possibly-cached NodeData chain).
    drop(root);
}

#[test]
fn cursor_free_via_for_each_mirrors_cyrs_pattern() {
    // Closely mirrors `crates/cyrs-hir/src/lower.rs:478` style:
    // `node.children().for_each(|c| { ... })`.
    let mut b = GreenNodeBuilder::new();
    b.start_node(SyntaxKind(0));
    for i in 0..16u32 {
        b.start_node(SyntaxKind(1));
        b.start_node(SyntaxKind(3));
        b.token(SyntaxKind(2), &format!("inner{i}"));
        b.finish_node();
        b.finish_node();
    }
    b.finish_node();
    let green = b.finish();

    let root = SyntaxNode::new_root(green);
    let mut sum = 0u32;
    root.children().for_each(|child| {
        sum += u32::from(child.text_range().len());
        // Recurse one level: touch the grandchild then let it drop.
        child.children().for_each(|gc| {
            sum += u32::from(gc.text_range().len());
        });
    });
    assert!(sum > 0);
}
