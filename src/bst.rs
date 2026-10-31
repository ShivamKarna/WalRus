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
}
