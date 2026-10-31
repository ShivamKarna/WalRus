pub struct Bst {
    root: Option<Box<Node>>,
    len: usize,
}

pub struct Node {
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
}

// returns true if a new node was added, false if an existing key was updated
pub fn insert_node(node: &mut Option<Box<Node>>, key: &[u8], val: &[u8]) -> bool {
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
}
