use std::iter::Peekable;

pub trait SortedMergeable<T, A: Iterator<Item=T>, B: Iterator<Item=T>> {
    fn sorted_merge_with(self, other: B) -> SortedMerge<Peekable<A>, Peekable<B>>;
}

impl<T, A: Iterator<Item=T>, B: Iterator<Item=T>> SortedMergeable<T, A, B> for A {
    fn sorted_merge_with(self, other: B) -> SortedMerge<Peekable<A>, Peekable<B>> {
        SortedMerge{left: self.peekable(), right: other.peekable()}
    }
}

#[derive(Clone)]
pub struct SortedMerge<A, B> {
    left: A,
    right: B,
}

impl<T: PartialOrd, A: Iterator<Item=T>, B: Iterator<Item=T>> Iterator for SortedMerge<Peekable<A>, Peekable<B>> {
    type Item = T;

    fn next(&mut self) -> Option<T> {
        match self.left.peek().map(|l| (l, self.right.peek())) {
            Some((_left, None)) => self.left.next(),
            Some((left, Some(right))) if left <= right => self.left.next(),
            _ => self.right.next(),
        }
    }
}
