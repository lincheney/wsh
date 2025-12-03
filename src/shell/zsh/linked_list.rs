use std::marker::PhantomData;
use std::pin::Pin;
use std::ptr::null_mut;

const EMPTY_NODE: zsh_sys::linknode = zsh_sys::linknode{
    next: null_mut(),
    prev: null_mut(),
    dat: null_mut(),
};

pub struct LinkedList<'a, T: ?Sized> {
    nodes: Pin<Box<[zsh_sys::linknode]>>,
    _phantom: PhantomData<&'a T>,
}

impl<'a, T: ?Sized> LinkedList<'a, T> {

    pub fn new<I: Iterator<Item=&'a T>>(size: usize, iter: I) -> Self {

        let mut nodes = Box::into_pin(vec![EMPTY_NODE; size].into_boxed_slice());

        for (item, node) in iter.zip(nodes.as_mut().iter_mut()) {
            node.dat = &raw const item as _;
        }

        for i in 0..nodes.len()-1 {
            nodes[i].next = &raw const nodes[i+1] as _;
            nodes[i+1].prev = &raw const nodes[i] as _;
        }

        Self {
            nodes,
            _phantom: PhantomData,
        }
    }

    pub fn as_linkroot(&self) -> zsh_sys::linkroot {
        zsh_sys::linkroot{
            list: zsh_sys::linklist{
                first: self.nodes.first().map_or(null_mut(), |x| &raw const x as _),
                last: self.nodes.last().map_or(null_mut(), |x| &raw const x as _),
                flags: 0,
            }
        }
    }
}
