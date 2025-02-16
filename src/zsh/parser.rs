use std::ffi::{CString, CStr};
use std::ops::Range;
use std::ptr::null_mut;
use bstr::{BStr, ByteSlice};

#[derive(Debug, Copy, Clone)]
pub enum StringType {
    Double,
    Single,
    Dollar, // $'...'
}

#[derive(Debug, Copy, Clone)]
pub enum TokenKind {
    Generic,
    String(StringType),
    CommandSeparator,
    // CommandSubstitution,
    Subshell(bool),
    Block(bool),
}

#[derive(Debug)]
pub struct Token {
    range: Range<usize>,
    kind: TokenKind,
}

impl Token {
    fn new(string: &BStr, range: Range<usize>, _prev_kind: TokenKind) -> Self {
        let kind = match string.as_bytes() {
            b";" | b"&" | b"&&" | b"|" | b"||"  => TokenKind::CommandSeparator,
            b"(" => TokenKind::Subshell(true),
            b")" => TokenKind::Subshell(false),
            b"{" => TokenKind::Block(true),
            b"}" => TokenKind::Block(false),
            s if s.starts_with(b"\"") => TokenKind::String(StringType::Double),
            s if s.starts_with(b"'") => TokenKind::String(StringType::Single),
            s if s.starts_with(b"$'") => TokenKind::String(StringType::Dollar),
            _ => TokenKind::Generic,
        };

        Self{
            range,
            kind,
        }
    }
}

pub fn parse(mut cmd: &BStr) -> (bool, Vec<Token>) {
    // we add some at the end to detect if the command line is actually complete
    let dummy = b" x";
    let mut c_cmd = cmd.to_vec();
    c_cmd.extend(dummy);
    let ptr = super::metafy(&c_cmd);

    let flags = zsh_sys::LEXFLAGS_ACTIVE | zsh_sys::LEXFLAGS_COMMENTS_KEEP;
    let split: Vec<_> = unsafe {
        let result = zsh_sys::bufferwords(null_mut(), ptr, null_mut(), flags as _);
        // these strings are allocated on the zsh arena
        super::iter_linked_list(result).map(|ptr| super::unmetafy(ptr as _)).collect()
    };

    // if the command is syntactically complete, then the last token should be a standalone 'x'
    let mut complete = split.last().is_some_and(|x| x == b"x");

    let mut prev_kind = TokenKind::Generic;
    let num_tokens = split.len();
    let tokens: Vec<_> = split
        .iter()
        .enumerate()
        .filter_map(|(i, token)| {
            let token = BStr::new(if i != num_tokens - 1 {
                *token
            } else if complete {
                // skip the last complete token
                return None
            } else {
                // skip the dummy text
                &token[..token.len() - dummy.len()]
            });

            let start = cmd.find(token).unwrap();
            let end = start + token.len();
            cmd = &cmd[end..];
            let token = Token::new(token, start..end, prev_kind);
            prev_kind = token.kind;
            Some(token)
        }).collect();

    if complete {
        // check if brackets all match
        let mut stack = vec![];
        for t in tokens.iter() {
            match t.kind {
                TokenKind::Subshell(true) => { stack.push(true); },
                TokenKind::Block(true) => { stack.push(true); },
                TokenKind::Subshell(false) if stack.last() == Some(&true) => { stack.pop(); },
                TokenKind::Block(false) if stack.last() == Some(&false) => { stack.pop(); },
                TokenKind::Subshell(false) | TokenKind::Block(false) => {
                    complete = false;
                    break
                },
                _ => (),
            }
        }
        complete = complete && stack.is_empty();
    }

    (complete, tokens)
}
