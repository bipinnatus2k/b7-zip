use crate::*;
use vfs::{VfsNode, next_node_id};
use vfs::attr::AttrValue;
use vfs::attr;

fn file_tree(items: &[(&str, u64)]) -> vfs::Tree {
    let mut tree = vfs::Tree::new(next_node_id());
    tree.insert_node(VfsNode::new(tree.root(), None, "", true)).unwrap();
    for (path, size) in items {
        let mut parts = path.split('/');
        let name = parts.next_back().unwrap();
        let parent_path = parts.collect::<Vec<_>>().join("/");
        let parent = if parent_path.is_empty() {
            tree.root()
        } else {
            // ensure nested dirs exist
            let mut dir_id = tree.root();
            for part in parent_path.split('/') {
                let existing = tree
                    .children(dir_id)
                    .unwrap_or(&[])
                    .iter()
                    .find(|&&id| tree.node(id).is_some_and(|n| n.name == part && n.is_directory))
                    .copied();
                dir_id = match existing {
                    Some(id) => id,
                    None => {
                        let id = next_node_id();
                        let node = VfsNode::new(id, Some(dir_id), part, true);
                        tree.insert_node(node).unwrap();
                        id
                    }
                };
            }
            dir_id
        };
        let mut node = VfsNode::new(next_node_id(), Some(parent), name, false);
        node.set_attr(attr::SIZE, AttrValue::UInt(*size));
        node.set_attr(attr::CRC, AttrValue::UInt(*size as u64));
        tree.insert_node(node).unwrap();
    }
    tree
}

#[test]
fn identical_trees_produce_empty_report() {
    let a = file_tree(&[("a.txt", 3), ("dir/b.txt", 5)]);
    let b = file_tree(&[("a.txt", 3), ("dir/b.txt", 5)]);
    let report = tree_vs_tree(&a, &b);
    assert!(report.is_empty(), "{:?}", report.entries);
}

#[test]
fn added_removed_modified_are_reported() {
    let base = file_tree(&[("kept.txt", 3), ("gone.txt", 4), ("changed.txt", 5)]);
    let working = file_tree(&[("kept.txt", 3), ("changed.txt", 9), ("new.txt", 1)]);
    let mut report = tree_vs_tree(&base, &working);

    let by_path = |report: &DiffReport, path: &str| -> DiffEntry {
        report
            .entries
            .iter()
            .find(|e| e.path == path)
            .cloned()
            .unwrap_or_else(|| panic!("missing {path} in {:?}", report.entries))
    };
    assert_eq!(by_path(&report, "gone.txt").kind, DiffKind::Removed);
    assert_eq!(by_path(&report, "new.txt").kind, DiffKind::Added);
    let changed = by_path(&report, "changed.txt");
    assert_eq!(changed.kind, DiffKind::Modified);
    assert_eq!(changed.base_size, Some(5));
    assert_eq!(changed.working_size, Some(9));
    assert_eq!(changed.letter(), "M");

    // Sorted by path for a stable display order.
    let paths: Vec<&str> = report.entries.iter().map(|e| e.path.as_str()).collect();
    let mut sorted = paths.clone();
    sorted.sort();
    assert_eq!(paths, sorted);
}

#[test]
fn crc_difference_flags_modified_even_with_equal_size() {
    let mut base = file_tree(&[("same-size.txt", 10)]);
    let mut working = file_tree(&[("same-size.txt", 10)]);
    let base_id = base.resolve_path("same-size.txt").unwrap();
    let working_id = working.resolve_path("same-size.txt").unwrap();
    base.node_mut(base_id).unwrap().set_attr(attr::CRC, AttrValue::UInt(1));
    working.node_mut(working_id).unwrap().set_attr(attr::CRC, AttrValue::UInt(2));
    let report = tree_vs_tree(&base, &working);
    assert_eq!(report.len(), 1);
    assert_eq!(report.entries[0].kind, DiffKind::Modified);
}

#[test]
fn equal_bytes_compare_same() {
    let bytes = b"hello\nworld\n";
    assert_eq!(compare_bytes(bytes, bytes), ContentDiff::Same);
}

#[test]
fn text_diff_rows_pair_removals_and_additions() {
    let base = b"alpha\nbeta\ngamma\n";
    let working = b"alpha\nBETA\ngamma\n";
    let ContentDiff::Text(rows) = compare_bytes(base, working) else {
        panic!("expected text diff");
    };
    let kinds: Vec<LineKind> = rows.iter().map(|r| r.kind).collect();
    assert_eq!(
        kinds,
        vec![
            LineKind::Context,
            LineKind::Changed,
            LineKind::Context,
        ],
        "rows: {rows:?}"
    );
    let changed = &rows[1];
    assert_eq!(changed.left.as_ref().unwrap().1, "beta\n");
    assert_eq!(changed.right.as_ref().unwrap().1, "BETA\n");
}

#[test]
fn pure_addition_has_none_on_the_left() {
    let base = b"one\n";
    let working = b"one\ntwo\nthree\n";
    let ContentDiff::Text(rows) = compare_bytes(base, working) else {
        panic!("expected text diff");
    };
    assert_eq!(rows[0].kind, LineKind::Context);
    assert_eq!(rows[1].kind, LineKind::Added);
    assert!(rows[1].left.is_none());
    assert_eq!(rows[1].right.as_ref().unwrap().1, "two\n");
    assert_eq!(rows[2].kind, LineKind::Added);
    assert_eq!(rows[2].right.as_ref().unwrap().1, "three\n");
}

#[test]
fn pure_removal_has_none_on_the_right() {
    let base = b"one\ntwo\nthree\n";
    let working = b"one\n";
    let ContentDiff::Text(rows) = compare_bytes(base, working) else {
        panic!("expected text diff");
    };
    assert_eq!(rows[0].kind, LineKind::Context);
    assert_eq!(rows[1].kind, LineKind::Removed);
    assert!(rows[1].right.is_none());
    assert_eq!(rows[1].left.as_ref().unwrap().1, "two\n");
    assert_eq!(rows[2].kind, LineKind::Removed);
    assert_eq!(rows[2].left.as_ref().unwrap().1, "three\n");
}

#[test]
fn binary_content_degrades_to_digest_card() {
    let base = [0u8, 1, 2, 0, 3];
    let working = [0u8, 1, 2, 0, 4];
    let ContentDiff::Binary { base: b, working: w } = compare_bytes(&base, &working) else {
        panic!("expected binary diff");
    };
    assert_eq!(b.size, 5);
    assert_eq!(w.size, 5);
    assert_ne!(b.md5, w.md5);
}

#[test]
fn nul_byte_only_in_one_side_still_binary() {
    // Text on both sides is required for a line diff; a NUL on either side
    // forces the digest card.
    let base = b"text\n";
    let working = b"text\0\n";
    assert!(matches!(
        compare_bytes(base, working),
        ContentDiff::Binary { .. }
    ));
}

#[test]
fn text_detection_heuristics() {
    assert!(is_probably_text(b"plain ascii\n"));
    assert!(is_probably_text(b"\xe4\xb8\xad\xe6\x96\x87 utf8\n"));
    assert!(!is_probably_text(b"bin\0ary"));
    assert!(is_probably_text(b""));
}
