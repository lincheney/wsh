use std::marker::PhantomData;
use std::pin::Pin;
use std::ptr::null_mut;
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
                first: self.nodes.first().map_or(null_mut(), |x| x as *const _ as _),
                last: self.nodes.last().map_or(null_mut(), |x| x as *const _ as _),
                flags: 0,
            }
        }
    }
}

pub fn iter_linklist(list: zsh_sys::LinkList) -> impl Iterator<Item=*mut c_void> {
    unsafe {
        let mut node = list.as_mut().and_then(|list| list.list.first.as_mut());
        std::iter::from_fn(move || {
            let n = node.take()?;
            node = n.next.as_mut();
            Some(n.dat)
        })
    }
}

pub fn rev_iter_linklist(list: zsh_sys::LinkList) -> impl Iterator<Item=*mut c_void> {
    unsafe {
        let first = list.as_mut().map(|list| list.list.first);
        let mut node = list.as_mut().and_then(|list| list.list.last.as_mut());
        std::iter::from_fn(move || {
            let n = node.take()?;
            if n.prev == first.unwrap() {
                node = None;
            } else {
                node = n.prev.as_mut();
            }
            Some(n.dat)
        })
    }
}
