#[derive(Default)]
pub struct Bst {
    root: Option<Box<Node>>,
    len: usize,
}

struct Node {
    key: Vec<u8>,
    val: Vec<u8>,
    left: Option<Box<Node>>,
    right: Option<Box<Node>>,
}

impl Bst {
    pub fn new() -> Self {
        Bst { root: None, len: 0 }
    }

    pub fn insert(&mut self, key: &[u8], val: &[u8]) {
        if insert_node(&mut self.root, key, val) {
            self.len += 1;
        }
    }

    pub fn get(&self, key: &[u8]) -> Option<&[u8]> {
        let mut current = &self.root;

        while let Some(node) = current {
            match key.cmp(node.key.as_slice()) {
                std::cmp::Ordering::Greater => current = &node.right,
                std::cmp::Ordering::Less => current = &node.left,
                std::cmp::Ordering::Equal => return Some(node.val.as_slice()),
            }
        }

        None
    }

    // returns true if the key was removed
    pub fn remove(&mut self, key: &[u8]) -> bool {
        if remove_node(&mut self.root, key) {
            self.len -= 1;
            true
        } else {
            false
        }
    }
}

// returns true if a new node was added, false if an existing key was updated
fn insert_node(node: &mut Option<Box<Node>>, key: &[u8], val: &[u8]) -> bool {
    match node {
        None => {
            *node = Some(Box::new(Node {
                key: key.to_vec(),
                val: val.to_vec(),
                left: None,
                right: None,
            }));
            true
        }

        Some(n) => match key.cmp(n.key.as_slice()) {
            std::cmp::Ordering::Less => insert_node(&mut n.left, key, val),
            std::cmp::Ordering::Greater => insert_node(&mut n.right, key, val),
            std::cmp::Ordering::Equal => {
                n.val = val.to_vec();
                false
            }
        },
    }
}

fn remove_node(node: &mut Option<Box<Node>>, key: &[u8]) -> bool {
    let ord = match node {
        None => return false,
        Some(n) => key.cmp(n.key.as_slice()),
    };

    match ord {
        std::cmp::Ordering::Less => remove_node(&mut node.as_mut().unwrap().left, key),
        std::cmp::Ordering::Greater => remove_node(&mut node.as_mut().unwrap().right, key),
        // this has the real work
        std::cmp::Ordering::Equal => {
            let mut boxed = node.take().unwrap();
            if boxed.left.is_none() {
                // no left child: right child (or nothing) takes my place
                *node = boxed.right.take();
            } else if boxed.right.is_none() {
                // no right child: left child takes my place
                *node = boxed.left.take();
            } else {
                let (k, v) = pop_min(&mut boxed.right).unwrap();
                boxed.key = k;
                boxed.val = v;
                *node = Some(boxed);
            }
            true
        }
    }
}

fn pop_min(node: &mut Option<Box<Node>>) -> Option<(Vec<u8>, Vec<u8>)> {
    if node.as_ref()?.left.is_some() {
        // keep going left is there is something smaller
        pop_min(&mut node.as_mut().unwrap().left)
    } else {
        // this is the min node
        let boxed = node.take()?;
        let Node {
            key, val, right, ..
        } = *boxed;

        *node = right;
        Some((key, val))
    }
}

#[cfg(test)]
mod test {

    use super::*;

    #[test]
    fn insert_overwrites_existing_key() {
        let mut t = Bst::new();

        t.insert(b"k", b"1");
        t.insert(b"k", b"2");

        assert_eq!(t.len, 1);
        assert_eq!(t.get(b"k"), Some(b"2".as_slice()));
    }

    #[test]
    fn insert_writes_smaller_left_larger_right() {
        let mut t = Bst::new();

        t.insert(b"b", b"1");
        t.insert(b"a", b"2");
        t.insert(b"c", b"3");

        let root = t.root.as_ref().unwrap();

        assert_eq!(root.key, b"b".to_vec());
        assert_eq!(root.left.as_ref().unwrap().key, b"a");
        assert_eq!(root.right.as_ref().unwrap().key, b"c");

        assert_eq!(t.len, 3);
    }

    #[test]
    fn get_finds_keys_on_both_sides() {
        let mut t = Bst::new();
        t.insert(b"b", b"1");
        t.insert(b"a", b"2");
        t.insert(b"c", b"3");

        assert_eq!(t.get(b"a"), Some(b"2".as_slice()));
        assert_eq!(t.get(b"b"), Some(b"1".as_slice()));
        assert_eq!(t.get(b"c"), Some(b"3".as_slice()));
        assert_eq!(t.get(b"zzz"), None);
    }

    #[test]
    fn get_on_empty_tree_is_none() {
        assert_eq!(Bst::new().get(b"x"), None);
    }

    fn build(keys: &[&str]) -> Bst {
        let mut t = Bst::new();
        for k in keys {
            t.insert(k.as_bytes(), k.as_bytes());
        }
        t
    }

    #[test]
    fn remove_missing_key_changes_nothing() {
        let mut t = build(&["b", "a", "c"]);

        assert!(!t.remove(b"zzz"));
        assert_eq!(t.len, 3);
        assert!(!Bst::new().remove(b"x"));
    }

    #[test]
    fn remove_leaf_and_one_child_nodes() {
        // a is a leaf, c has only a left child (b)
        let mut t = build(&["d", "c", "b", "e", "a"]);

        assert!(t.remove(b"a"));
        assert!(t.remove(b"c"));
        assert_eq!(t.len, 3);
        assert_eq!(t.get(b"a"), None);
        assert_eq!(t.get(b"c"), None);
        assert!(t.get(b"b").is_some() && t.get(b"d").is_some() && t.get(b"e").is_some());
    }

    #[test]
    fn remove_two_children_with_deep_successor() {
        let mut t = build(&["50", "30", "70", "60", "65"]);

        assert!(t.remove(b"50"));
        assert_eq!(t.len, 4);
        assert_eq!(t.get(b"50"), None);
        assert_eq!(t.root.as_ref().unwrap().key, b"60".to_vec());

        for k in ["30", "60", "65", "70"] {
            assert!(t.get(k.as_bytes()).is_some(), "lost {k}"); // fail with msg "lost {k}", if k
        }
    }

    #[test]
    fn remove_everything_one_by_one() {
        let keys = ["d", "b", "f", "a", "c", "e", "g"];
        let mut t = build(&keys);

        for (i, k) in keys.iter().enumerate() {
            assert!(t.remove(k.as_bytes()), "failed to remove {k}");
            assert_eq!(t.len, keys.len() - i - 1);
            for rest in &keys[i + 1..] {
                assert!(
                    t.get(rest.as_bytes()).is_some(),
                    "lost {rest} after removing {k}"
                );
            }
        }
        assert!(t.root.is_none());
    }
}
