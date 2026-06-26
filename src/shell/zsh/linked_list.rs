use std::marker::PhantomData;
use std::pin::Pin;
use std::ptr::{null_mut, NonNull};
use std::os::raw::c_void;

pub struct LinkedList<'a, T: ?Sized> {
    nodes: Pin<Box<[zsh_sys::linknode]>>,
    _phantom: PhantomData<&'a T>,
}

impl<'a, T: ?Sized> LinkedList<'a, T> {

    pub fn new<I: Iterator<Item=&'a T>>(iter: I) -> Self {
        Self::new_from_ptrs(iter.map(|x| x as _))
    }

    pub fn new_from_ptrs<I: Iterator<Item=*const T>>(iter: I) -> Self {

        let nodes: Vec<_> = iter.map(|ptr| zsh_sys::linknode {
            next: null_mut(),
            prev: null_mut(),
            dat: ptr as _,
        }).collect();

        let mut nodes = Box::into_pin(nodes.into_boxed_slice());

        for i in 0..nodes.len()-1 {
            nodes[i].next = (&raw const nodes[i+1]).cast_mut();
            nodes[i+1].prev = (&raw const nodes[i]).cast_mut();
        }

        Self {
            nodes,
            _phantom: PhantomData,
        }
    }

    pub fn as_linkroot(&self) -> zsh_sys::linkroot {
        zsh_sys::linkroot{
            list: zsh_sys::linklist{
                first: self.nodes.first().map_or(null_mut(), |x| x as *const _ as _),
                last: self.nodes.last().map_or(null_mut(), |x| x as *const _ as _),
                flags: 0,
            }
        }
    }
}

pub struct Iter {
    first: Option<NonNull<zsh_sys::linknode>>,
    last: Option<NonNull<zsh_sys::linknode>>,
}
impl Iterator for Iter {
    type Item = *mut c_void;
    fn next(&mut self) -> Option<Self::Item> {
        let node = self.first.take()?;
        let node = unsafe{ node.as_ref() };
        if self.first == self.last {
            self.first = None;
        } else {
            self.first = NonNull::new(node.next);
        }
        Some(node.dat)
    }
}

impl DoubleEndedIterator for Iter {
    // Required method
    fn next_back(&mut self) -> Option<Self::Item> {
        let node = self.last.take()?;
        let node = unsafe{ node.as_ref() };
        if self.first == self.last {
            self.last = None;
        } else {
            self.last = NonNull::new(node.prev);
        }
        Some(node.dat)
    }
}

pub fn iter_linklist(list: zsh_sys::LinkList) -> Iter {
    unsafe {
        let list = list.as_ref();
        Iter{
            first: list.and_then(|list| NonNull::new(list.list.first)),
            last: list.and_then(|list| NonNull::new(list.list.last)),
        }
    }
}
